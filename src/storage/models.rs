use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Source enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum MangaSource {
    #[default]
    Local,
    WeebCentral {
        series_url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum ChapterSource {
    #[default]
    Local,
    WeebCentral {
        chapter_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MangaId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChapterId(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MangaMeta {
    pub id: MangaId,
    pub title: String,
    pub mangadex_id: Option<String>,
    #[serde(default)]
    pub source: MangaSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterMeta {
    pub id: ChapterId,
    pub manga_id: MangaId,
    /// Fractional chapter number used for ordering (e.g. 1.5 for a bonus chapter).
    pub chapter_number: f32,
    pub tankobon_number: Option<u32>,
    pub filename: String,
    pub page_count: u32,
    #[serde(default)]
    pub source: ChapterSource,
    /// CDN image URLs for WeebCentral chapters. Empty for Local chapters.
    #[serde(default)]
    pub page_urls: Vec<String>,
}

/// An entry in the interleaved library view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LibraryEntry {
    /// A physical volume that groups one or more chapters.
    Tankobon {
        number: u32,
        chapters: Vec<ChapterMeta>,
    },
    /// A chapter that does not belong to any tankobon volume.
    LoneChapter(ChapterMeta),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadingProgress {
    pub manga_id: MangaId,
    pub chapter_id: ChapterId,
    pub page: usize,
}

/// Stored in `localStorage` so the app can restore the last session state
/// across page reloads without hitting IndexedDB.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LastOpened {
    Shelf,
    Library { manga_id: String },
    Reader { manga_id: String, chapter_id: String, page: usize },
}

/// Build the interleaved list of [`LibraryEntry`] values from a flat list of
/// chapters.
///
/// Algorithm:
/// 1. Sort chapters by `chapter_number` (ascending).
/// 2. Walk through the sorted list.  Chapters that carry the same
///    `tankobon_number` are accumulated into a [`LibraryEntry::Tankobon`]
///    group; chapters without a `tankobon_number` are emitted immediately as
///    [`LibraryEntry::LoneChapter`].
/// 3. A pending tankobon group is flushed whenever a chapter with a *different*
///    (or absent) tankobon number is encountered, preserving interleaved order.
pub fn build_library_entries(mut chapters: Vec<ChapterMeta>) -> Vec<LibraryEntry> {
    // 1. Sort by chapter_number.  Use total_cmp so NaN sorts to the end.
    chapters.sort_by(|a, b| a.chapter_number.total_cmp(&b.chapter_number));

    let mut entries: Vec<LibraryEntry> = Vec::new();
    // Accumulator for the current tankobon group: (volume number, chapters).
    let mut pending_tankobon: Option<(u32, Vec<ChapterMeta>)> = None;

    for chapter in chapters {
        match chapter.tankobon_number {
            Some(vol) => {
                match pending_tankobon {
                    Some((current_vol, ref mut acc)) if current_vol == vol => {
                        // Same volume — keep accumulating.
                        acc.push(chapter);
                    }
                    _ => {
                        // Different volume (or no pending group): flush the old
                        // group first, then start a new one.
                        if let Some((number, group_chapters)) = pending_tankobon.take() {
                            entries.push(LibraryEntry::Tankobon {
                                number,
                                chapters: group_chapters,
                            });
                        }
                        pending_tankobon = Some((vol, vec![chapter]));
                    }
                }
            }
            None => {
                // Flush any pending tankobon group before emitting the lone
                // chapter so that the interleaved order is preserved.
                if let Some((number, group_chapters)) = pending_tankobon.take() {
                    entries.push(LibraryEntry::Tankobon {
                        number,
                        chapters: group_chapters,
                    });
                }
                entries.push(LibraryEntry::LoneChapter(chapter));
            }
        }
    }

    // Flush any remaining tankobon group.
    if let Some((number, group_chapters)) = pending_tankobon.take() {
        entries.push(LibraryEntry::Tankobon {
            number,
            chapters: group_chapters,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chapter(id: &str, chapter_number: f32, tankobon_number: Option<u32>) -> ChapterMeta {
        ChapterMeta {
            id: ChapterId(id.to_string()),
            manga_id: MangaId("m1".to_string()),
            chapter_number,
            tankobon_number,
            filename: format!("{}.cbz", id),
            page_count: 20,
            source: crate::storage::models::ChapterSource::Local,
            page_urls: vec![],
        }
    }

    #[test]
    fn test_all_lone_chapters() {
        let chapters = vec![
            make_chapter("c3", 3.0, None),
            make_chapter("c1", 1.0, None),
            make_chapter("c2", 2.0, None),
        ];
        let entries = build_library_entries(chapters);
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], LibraryEntry::LoneChapter(c) if c.id.0 == "c1"));
        assert!(matches!(&entries[1], LibraryEntry::LoneChapter(c) if c.id.0 == "c2"));
        assert!(matches!(&entries[2], LibraryEntry::LoneChapter(c) if c.id.0 == "c3"));
    }

    #[test]
    fn test_all_tankobon() {
        let chapters = vec![
            make_chapter("c2", 2.0, Some(1)),
            make_chapter("c1", 1.0, Some(1)),
            make_chapter("c3", 3.0, Some(2)),
        ];
        let entries = build_library_entries(chapters);
        assert_eq!(entries.len(), 2);
        match &entries[0] {
            LibraryEntry::Tankobon { number, chapters } => {
                assert_eq!(*number, 1);
                assert_eq!(chapters.len(), 2);
            }
            _ => panic!("expected Tankobon"),
        }
        match &entries[1] {
            LibraryEntry::Tankobon { number, chapters } => {
                assert_eq!(*number, 2);
                assert_eq!(chapters.len(), 1);
            }
            _ => panic!("expected Tankobon"),
        }
    }

    #[test]
    fn test_interleaved() {
        // vol 1: ch1, ch2 | lone: ch2.5 | vol 2: ch3
        let chapters = vec![
            make_chapter("c1", 1.0, Some(1)),
            make_chapter("c2", 2.0, Some(1)),
            make_chapter("c2_5", 2.5, None),
            make_chapter("c3", 3.0, Some(2)),
        ];
        let entries = build_library_entries(chapters);
        assert_eq!(entries.len(), 3);
        assert!(matches!(
            &entries[0],
            LibraryEntry::Tankobon { number: 1, .. }
        ));
        assert!(matches!(&entries[1], LibraryEntry::LoneChapter(c) if c.id.0 == "c2_5"));
        assert!(matches!(
            &entries[2],
            LibraryEntry::Tankobon { number: 2, .. }
        ));
    }
}
