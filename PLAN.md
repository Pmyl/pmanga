# PManga - Feature Plan

## Pages and Navigation

Four pages in the app:

- **Shelf** - list of all manga series
- **Library** - volumes and lone chapters for one manga series
- **Reader** - full-screen, page-by-page reading
- **Settings** - configure gamepad button mappings

### Startup Behaviour
- If the user has never read anything: open on Shelf
- If the user has previously read something: open directly on the last page they had open in the Reader

---

## Shelf Page

- Grid of manga series cards, each showing:
  - Cover thumbnail (first page of first chapter of first tankobon/volume)
  - Manga title
  - Overall reading progress across all volumes
- Button to import manga (manga name guessed from filename, editable; MangaDex queried to confirm identity)
- Tapping a manga card opens its Library page

---

## Library Page (per manga)

- Displays volumes (tankobons) and lone chapters interleaved in order, e.g.:
  `| Tankobon 1 | Chapter 9 | Tankobon 2 |`
- Each card shows:
  - Cover thumbnail (first page of first chapter in that volume/chapter)
  - Volume number or chapter number
  - Reading progress (% read + pages read / total pages)
- Button to import chapters for this manga (manga name is pre-filled, not guessed)
- Each volume/chapter card can be deleted (with confirmation) to free space
- Tapping a card opens the Reader at the last read position, or from the beginning

---

## Reader Page

- Full-screen, one page at a time
- Page always fits entirely on screen (both width and height), no cropping - equivalent to object-fit: contain
- Works in portrait and landscape; landscape just gives a wider viewport for the same single page
- No page-turn animation or transition - instant swap
- Tap/click zones:
  - Left third: previous page
  - Right third: next page
  - Top strip: toggle info overlay
- Chapter boundary behaviour:
  - Previous page on the first page of a chapter: jump to last page of previous chapter
  - Next page on the last page of a chapter: jump to first page of next chapter
- Reading position is saved to IndexedDB on every page turn

### Info Overlay (top bar)
- Toggled by tapping the top strip or a dedicated gamepad button
- Shows: manga name, volume number, chapter number, filename, current page / total pages in chapter
- Contains a Back to Library button
- Tapping the top strip again (or pressing the toggle button again) hides it
- While overlay is visible, a dedicated gamepad back button navigates back to Library

---

## Import Flow

### Entry Points
1. **From Shelf** - import button; manga name is guessed heuristically from filename and is editable by the user
2. **From Library** - import button; manga name is pre-filled as the current manga, not guessed

### Steps
1. User selects one or more PDF files, or a ZIP (one level deep) containing multiple PDFs
2. App proposes for each file:
   - Manga name (guessed from filename if from Shelf; pre-filled if from Library)
   - Chapter number (heuristic: extract numbers from filename)
3. If entering from Shelf: app queries MangaDex API with the guessed manga name and shows a list of matches; user selects the correct one (or skips if not found or offline)
4. User sees a review table: `filename | manga name | chapter number | tankobon number`
   - All fields are editable
   - Tankobon number is auto-filled via MangaDex chapter-to-volume data if a manga was confirmed, OR looked up in `tankobon_db.csv` (offline fallback/override), OR left blank for the user to fill manually
5. User confirms; pages are rendered from PDFs via PDF.js and stored as image blobs in IndexedDB

### tankobon_db.csv
- Format: `manga_title, chapter_number, tankobon_number`
- Fetched from the root of the web app at import time
- Used as offline fallback when MangaDex is unavailable or the manga is not found there
- A minimal example file is included in the repo for testing

### Unmapped Chapters
- Chapters with no tankobon assigned are displayed as individual cards in the Library
- They are interleaved between tankobon cards based on chapter number order

---

## Storage

- **IndexedDB** - all manga data: page image blobs, metadata (manga title, chapter number, tankobon number, page count), reading progress per chapter
- **localStorage** - last-opened position (manga + chapter + page) for startup redirect, and gamepad button mappings

---

## Input Abstraction

All user actions are expressed as abstract actions. Multiple input sources map into the same action layer:

| Action        | Touch / Mouse     | Gamepad               |
|---------------|-------------------|-----------------------|
| NextPage      | Tap right third   | e.g. R1 / B           |
| PreviousPage  | Tap left third    | e.g. L1 / A           |
| ToggleOverlay | Tap top strip     | e.g. Select / View    |
| GoBack        | Button in overlay | Dedicated back button |
| Confirm       | Tap               | e.g. A / Cross        |

- Gamepad polling uses the browser Gamepad API
- The input layer is a single abstraction that both touch/mouse and gamepad feed into, making it easy to add new input types later

---

## Settings Page

- A simple list of every abstract action paired with its currently assigned gamepad button
- To remap an action: user presses the row's "remap" button, app enters listening mode for that action, user presses the desired gamepad button, mapping is saved immediately
- No graphical gamepad rendering - plain table: `Action | Current button | Remap button`
- Mappings are stored in localStorage
- A "Reset to defaults" button restores all mappings to the built-in defaults
- Accessible from the Shelf page (e.g. a settings icon in the corner)

---

## Technical Notes

- **PDF rendering**: PDF.js (Mozilla, Apache 2.0) via JS interop from Dioxus - runs entirely in the browser, no server needed
- **ZIP extraction**: handled client-side in the browser via JS interop
- **MangaDex API**: free, no authentication required for reads; queried only during import
- **Offline-first**: fully functional with no network after initial load; MangaDex queries are best-effort and gracefully skipped when unavailable
- **Mobile-first UI**: designed for portrait smartphone, also works correctly in landscape and on desktop