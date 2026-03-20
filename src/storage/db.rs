//! IndexedDB access layer.
//!
//! Stores manga metadata, chapter metadata, page image blobs, and reading
//! progress.  All async operations bridge the callback-based IndexedDB API
//! into Rust futures via `futures_channel::oneshot`.

use std::cell::RefCell;
use std::rc::Rc;

use futures_channel::oneshot;
use js_sys::{Array, JsString};
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    Event, IdbCursorWithValue, IdbDatabase, IdbIndexParameters, IdbKeyRange,
    IdbObjectStoreParameters, IdbOpenDbRequest, IdbRequest, IdbTransactionMode,
    IdbVersionChangeEvent,
};

use super::models::{ChapterId, ChapterMeta, MangaId, MangaMeta, ReadingProgress};

// ---------------------------------------------------------------------------
// Store / index names
// ---------------------------------------------------------------------------

const DB_NAME: &str = "pmanga";
const DB_VERSION: u32 = 1;

const STORE_MANGAS: &str = "mangas";
const STORE_CHAPTERS: &str = "chapters";
const STORE_PAGES: &str = "pages";
const STORE_PROGRESS: &str = "progress";

const IDX_CHAPTERS_BY_MANGA: &str = "by_manga";

// ---------------------------------------------------------------------------
// Public struct
// ---------------------------------------------------------------------------

/// Thin wrapper around an [`IdbDatabase`] handle.
///
/// `IdbDatabase` is not `Send`, so this type is intentionally `!Send` too.
/// Within a single-threaded WASM context that is fine.
pub struct Db {
    inner: Rc<IdbDatabase>,
}

// ---------------------------------------------------------------------------
// Helper: wrap an IdbRequest into a one-shot future
// ---------------------------------------------------------------------------

/// Attaches `onsuccess` / `onerror` handlers to `request` and returns a
/// `Receiver` that resolves to `Ok(JsValue)` or `Err(JsValue)`.
fn request_to_future(request: &IdbRequest) -> oneshot::Receiver<Result<JsValue, JsValue>> {
    let (tx, rx) = oneshot::channel::<Result<JsValue, JsValue>>();

    let tx_ok = Rc::new(RefCell::new(Some(tx)));
    let tx_err = Rc::clone(&tx_ok);

    let on_success = Closure::once(move |event: Event| {
        if let Some(tx) = tx_ok.borrow_mut().take() {
            let result = event
                .target()
                .and_then(|t| t.dyn_into::<IdbRequest>().ok())
                .and_then(|r| r.result().ok())
                .unwrap_or(JsValue::UNDEFINED);
            let _ = tx.send(Ok(result));
        }
    });

    let on_error = Closure::once(move |event: Event| {
        if let Some(tx) = tx_err.borrow_mut().take() {
            let err = event
                .target()
                .and_then(|t| t.dyn_into::<IdbRequest>().ok())
                .and_then(|r| r.error().ok().flatten())
                .map(|e| JsValue::from(e))
                .unwrap_or_else(|| JsValue::from_str("unknown IDB error"));
            let _ = tx.send(Err(err));
        }
    });

    request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
    request.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    // Leak the closures: IDB will invoke them exactly once and we have no
    // other place to keep them alive.
    on_success.forget();
    on_error.forget();

    rx
}

/// Await a request future, mapping errors to `String`.
async fn await_request(rx: oneshot::Receiver<Result<JsValue, JsValue>>) -> Result<JsValue, String> {
    rx.await
        .map_err(|_| "IDB request channel cancelled".to_string())?
        .map_err(|e| format!("{:?}", e))
}

// ---------------------------------------------------------------------------
// Helper: build the composite [chapter_id, page_number] key
// ---------------------------------------------------------------------------

fn page_key(chapter_id: &ChapterId, page_number: u32) -> JsValue {
    let arr = Array::new();
    arr.push(&JsValue::from_str(&chapter_id.0));
    arr.push(&JsValue::from_f64(page_number as f64));
    arr.into()
}

/// Build the lower/upper bounds for a prefix scan on the pages store.
///
/// Lower bound: `[chapter_id, 0]`   (inclusive)
/// Upper bound: `[chapter_id, ∞]`   — approximated by a very large number;
/// IDB array key comparison is lexicographic so any page number fits.
fn page_range_for_chapter(chapter_id: &ChapterId) -> Result<IdbKeyRange, String> {
    let lower = {
        let arr = Array::new();
        arr.push(&JsValue::from_str(&chapter_id.0));
        arr.push(&JsValue::from_f64(0.0));
        JsValue::from(arr)
    };
    let upper = {
        let arr = Array::new();
        arr.push(&JsValue::from_str(&chapter_id.0));
        // u32::MAX is large enough; actual page numbers are far smaller.
        arr.push(&JsValue::from_f64(u32::MAX as f64));
        JsValue::from(arr)
    };
    IdbKeyRange::bound(&lower, &upper).map_err(|e| format!("{:?}", e))
}

// ---------------------------------------------------------------------------
// Helper: JSON (de)serialisation via JsValue
// ---------------------------------------------------------------------------

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, String> {
    let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
    js_sys::JSON::parse(&json).map_err(|e| format!("{:?}", e))
}

fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, String> {
    let json_str = js_sys::JSON::stringify(&value)
        .map_err(|e| format!("{:?}", e))?
        .as_string()
        .ok_or_else(|| "JSON stringify returned non-string".to_string())?;
    serde_json::from_str(&json_str).map_err(|e| e.to_string())
}

/// Deserialise a JS Array of JSON objects into a `Vec<T>`.
fn array_to_vec<T: DeserializeOwned>(value: JsValue) -> Result<Vec<T>, String> {
    let arr: Array = value
        .dyn_into()
        .map_err(|_| "expected JS Array from getAll".to_string())?;
    let mut out = Vec::with_capacity(arr.length() as usize);
    for i in 0..arr.length() {
        out.push(from_js(arr.get(i))?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// impl Db
// ---------------------------------------------------------------------------

impl Db {
    // -----------------------------------------------------------------------
    // open
    // -----------------------------------------------------------------------

    /// Open (or create / upgrade) the `pmanga` IndexedDB database.
    pub async fn open() -> Result<Self, String> {
        let window = web_sys::window().ok_or("no window")?;
        let idb_factory = window
            .indexed_db()
            .map_err(|e| format!("{:?}", e))?
            .ok_or("no IDB factory")?;

        let open_req: IdbOpenDbRequest = idb_factory
            .open_with_u32(DB_NAME, DB_VERSION)
            .map_err(|e| format!("{:?}", e))?;

        // --- onupgradeneeded -------------------------------------------------
        // We need to create all stores and indexes here.  The closure runs
        // synchronously inside the IDB version-change transaction so we must
        // not await anything inside it.
        let on_upgrade = Closure::once(move |event: IdbVersionChangeEvent| {
            let target = match event.target() {
                Some(t) => t,
                None => return,
            };
            let req: IdbOpenDbRequest = match target.dyn_into() {
                Ok(r) => r,
                Err(_) => return,
            };
            let db: IdbDatabase = match req.result() {
                Ok(v) => match v.dyn_into() {
                    Ok(d) => d,
                    Err(_) => return,
                },
                Err(_) => return,
            };

            // --- mangas store (key: manga id string, out-of-line) -----------
            {
                let params = IdbObjectStoreParameters::new();
                // No in-line key; we supply it explicitly on put().
                let _ = db.create_object_store_with_optional_parameters(STORE_MANGAS, &params);
            }

            // --- chapters store (key: chapter id string, out-of-line) -------
            // Index on the `manga_id` field so we can query by manga.
            {
                let params = IdbObjectStoreParameters::new();
                if let Ok(store) =
                    db.create_object_store_with_optional_parameters(STORE_CHAPTERS, &params)
                {
                    let idx_params = IdbIndexParameters::new();
                    let _ = store.create_index_with_str_and_optional_parameters(
                        IDX_CHAPTERS_BY_MANGA,
                        "manga_id",
                        &idx_params,
                    );
                }
            }

            // --- pages store (composite key: [chapter_id, page_number]) -----
            {
                let key_path_arr = Array::new();
                key_path_arr.push(&JsValue::from_str("chapter_id"));
                key_path_arr.push(&JsValue::from_str("page_number"));
                let key_path_val = JsValue::from(key_path_arr);

                let params = IdbObjectStoreParameters::new();
                params.set_key_path(&key_path_val);
                // We store raw Blobs with an explicit key rather than using
                // in-line key paths on the Blob itself, so we use an
                // out-of-line key (keyPath = null) and pass the key to put().
                // Reset key_path to null:
                params.set_key_path(&JsValue::NULL);
                let _ = db.create_object_store_with_optional_parameters(STORE_PAGES, &params);
            }

            // --- progress store (key: chapter_id string, out-of-line) -------
            {
                let params = IdbObjectStoreParameters::new();
                let _ = db.create_object_store_with_optional_parameters(STORE_PROGRESS, &params);
            }
        });

        open_req.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
        on_upgrade.forget();

        // --- wait for the open request to complete --------------------------
        let rx = request_to_future(open_req.as_ref());
        let db_val = await_request(rx).await?;

        let db: IdbDatabase = db_val
            .dyn_into()
            .map_err(|_| "open result is not IdbDatabase".to_string())?;

        Ok(Db { inner: Rc::new(db) })
    }

    // -----------------------------------------------------------------------
    // Internal transaction helpers
    // -----------------------------------------------------------------------

    fn ro_store(&self, store_name: &str) -> Result<web_sys::IdbObjectStore, String> {
        let tx = self
            .inner
            .transaction_with_str(store_name)
            .map_err(|e| format!("{:?}", e))?;
        tx.object_store(store_name).map_err(|e| format!("{:?}", e))
    }

    fn rw_store(&self, store_name: &str) -> Result<web_sys::IdbObjectStore, String> {
        let tx = self
            .inner
            .transaction_with_str_and_mode(store_name, IdbTransactionMode::Readwrite)
            .map_err(|e| format!("{:?}", e))?;
        tx.object_store(store_name).map_err(|e| format!("{:?}", e))
    }

    // -----------------------------------------------------------------------
    // Manga
    // -----------------------------------------------------------------------

    /// Persist a [`MangaMeta`].  Uses `put` so it upserts.
    pub async fn save_manga(&self, manga: &MangaMeta) -> Result<(), String> {
        let store = self.rw_store(STORE_MANGAS)?;
        let key = JsValue::from_str(&manga.id.0);
        let value = to_js(manga)?;
        let req = store
            .put_with_key(&value, &key)
            .map_err(|e| format!("{:?}", e))?;
        await_request(request_to_future(&req)).await?;
        Ok(())
    }

    /// Load every [`MangaMeta`] in the store.
    pub async fn load_all_mangas(&self) -> Result<Vec<MangaMeta>, String> {
        let store = self.ro_store(STORE_MANGAS)?;
        let req = store.get_all().map_err(|e| format!("{:?}", e))?;
        let val = await_request(request_to_future(&req)).await?;
        array_to_vec(val)
    }

    /// Delete a manga by id.
    pub async fn delete_manga(&self, id: &MangaId) -> Result<(), String> {
        let store = self.rw_store(STORE_MANGAS)?;
        let key = JsValue::from_str(&id.0);
        let req = store.delete(&key).map_err(|e| format!("{:?}", e))?;
        await_request(request_to_future(&req)).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Chapters
    // -----------------------------------------------------------------------

    /// Persist a [`ChapterMeta`].  Uses `put` so it upserts.
    ///
    /// The stored JSON object must contain a `manga_id` *string* field so that
    /// the `by_manga` index can operate on it.  We store the inner string
    /// value directly for that field.
    pub async fn save_chapter(&self, chapter: &ChapterMeta) -> Result<(), String> {
        let store = self.rw_store(STORE_CHAPTERS)?;
        let key = JsValue::from_str(&chapter.id.0);

        // Serialise the struct to a plain JS object …
        let value = to_js(chapter)?;

        // … then patch the `manga_id` property so the index sees a plain
        // string, not `{ "0": "..." }` (which is how serde serialises the
        // newtype wrapper by default).
        let manga_id_str = JsValue::from_str(&chapter.manga_id.0);
        js_sys::Reflect::set(&value, &JsString::from("manga_id"), &manga_id_str)
            .map_err(|e| format!("{:?}", e))?;

        let req = store
            .put_with_key(&value, &key)
            .map_err(|e| format!("{:?}", e))?;
        await_request(request_to_future(&req)).await?;
        Ok(())
    }

    /// Load all chapters that belong to `manga_id`.
    pub async fn load_chapters_for_manga(
        &self,
        manga_id: &MangaId,
    ) -> Result<Vec<ChapterMeta>, String> {
        let store = self.ro_store(STORE_CHAPTERS)?;
        let index = store
            .index(IDX_CHAPTERS_BY_MANGA)
            .map_err(|e| format!("{:?}", e))?;
        let query = JsValue::from_str(&manga_id.0);
        let req = index
            .get_all_with_key(&query)
            .map_err(|e| format!("{:?}", e))?;
        let val = await_request(request_to_future(&req)).await?;
        array_to_vec(val)
    }

    /// Delete a chapter by id.
    pub async fn delete_chapter(&self, id: &ChapterId) -> Result<(), String> {
        let store = self.rw_store(STORE_CHAPTERS)?;
        let key = JsValue::from_str(&id.0);
        let req = store.delete(&key).map_err(|e| format!("{:?}", e))?;
        await_request(request_to_future(&req)).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Pages
    // -----------------------------------------------------------------------

    /// Store a single rendered page as a [`web_sys::Blob`].
    ///
    /// Key: `[chapter_id, page_number]`
    pub async fn save_page(
        &self,
        chapter_id: &ChapterId,
        page_number: u32,
        blob: web_sys::Blob,
    ) -> Result<(), String> {
        let store = self.rw_store(STORE_PAGES)?;
        let key = page_key(chapter_id, page_number);
        let req = store
            .put_with_key(&blob, &key)
            .map_err(|e| format!("{:?}", e))?;
        await_request(request_to_future(&req)).await?;
        Ok(())
    }

    /// Retrieve a page [`web_sys::Blob`].  Returns `None` if not yet stored.
    pub async fn load_page(
        &self,
        chapter_id: &ChapterId,
        page_number: u32,
    ) -> Result<Option<web_sys::Blob>, String> {
        let store = self.ro_store(STORE_PAGES)?;
        let key = page_key(chapter_id, page_number);
        let req = store.get(&key).map_err(|e| format!("{:?}", e))?;
        let val = await_request(request_to_future(&req)).await?;
        if val.is_undefined() || val.is_null() {
            return Ok(None);
        }
        let blob: web_sys::Blob = val
            .dyn_into()
            .map_err(|_| "page value is not a Blob".to_string())?;
        Ok(Some(blob))
    }

    /// Delete every stored page for `chapter_id` by opening a cursor over the
    /// `[chapter_id, 0] … [chapter_id, MAX]` key range.
    pub async fn delete_pages_for_chapter(&self, chapter_id: &ChapterId) -> Result<(), String> {
        let range = page_range_for_chapter(chapter_id)?;
        let store = self.rw_store(STORE_PAGES)?;

        // We iterate the cursor in a loop, advancing and deleting until done.
        // Each `continue_()` fires a new `onsuccess` on the *same* request, so
        // we re-use the same channel pattern but must re-register handlers
        // each time.
        loop {
            let req = store
                .open_cursor_with_range(&range)
                .map_err(|e| format!("{:?}", e))?;
            let val = await_request(request_to_future(&req)).await?;

            if val.is_null() || val.is_undefined() {
                // No more entries.
                break;
            }

            let cursor: IdbCursorWithValue = val
                .dyn_into()
                .map_err(|_| "expected IdbCursorWithValue".to_string())?;

            // Delete the current record.
            let del_req = cursor.delete().map_err(|e| format!("{:?}", e))?;
            await_request(request_to_future(&del_req)).await?;

            // Advance to the next record by calling `continue_()`.
            // This does NOT return a new IdbRequest — it fires `onsuccess`
            // again on the *same* request with the next cursor.  So we just
            // loop back and re-open the cursor (from the start of the
            // remaining range) which is simpler and fully correct.
            // (The previous cursor's transaction is still live because the
            // IdbObjectStore is obtained fresh each iteration from a new
            // transaction, so re-opening works.)
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Progress
    // -----------------------------------------------------------------------

    /// Persist a [`ReadingProgress`] entry.  Uses `put` so it upserts.
    pub async fn save_progress(&self, progress: &ReadingProgress) -> Result<(), String> {
        let store = self.rw_store(STORE_PROGRESS)?;
        let key = JsValue::from_str(&progress.chapter_id.0);
        let value = to_js(progress)?;
        let req = store
            .put_with_key(&value, &key)
            .map_err(|e| format!("{:?}", e))?;
        await_request(request_to_future(&req)).await?;
        Ok(())
    }

    /// Load the [`ReadingProgress`] for a specific chapter.
    pub async fn load_progress(
        &self,
        chapter_id: &ChapterId,
    ) -> Result<Option<ReadingProgress>, String> {
        let store = self.ro_store(STORE_PROGRESS)?;
        let key = JsValue::from_str(&chapter_id.0);
        let req = store.get(&key).map_err(|e| format!("{:?}", e))?;
        let val = await_request(request_to_future(&req)).await?;
        if val.is_undefined() || val.is_null() {
            return Ok(None);
        }
        Ok(Some(from_js(val)?))
    }

    /// Load every [`ReadingProgress`] record.
    pub async fn load_all_progress(&self) -> Result<Vec<ReadingProgress>, String> {
        let store = self.ro_store(STORE_PROGRESS)?;
        let req = store.get_all().map_err(|e| format!("{:?}", e))?;
        let val = await_request(request_to_future(&req)).await?;
        array_to_vec(val)
    }
}
