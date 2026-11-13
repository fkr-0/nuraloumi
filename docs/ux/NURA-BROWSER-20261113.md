# Nura Browser native toolbar: UX contract

The browser's controls are a NuraLoumi Cairo/Wayland layer-shell surface,
not a GTK window or HTML controls embedded in a webpage. The WPE webpage
remains a separate native WPEPlatform toplevel.

| Control | Touch/keyboard | Action |
| --- | --- | --- |
| Fixed horizontal tabs | Tap tab | Select independent WebKit tab |
| Address | Tap / Ctrl+L | Enter address editing mode |
| Address editing | Text / Backspace | Change bounded address buffer |
| Go | Enter in address mode / Go row | Open validated HTTP(S) URL |
| Back / Forward | Tap | Traverse selected tab's native history |
| Reload / Stop | Tap | Control outstanding load |
| New/close tab | Tap / Ctrl+T / Ctrl+W | Manage 1–6 real WebKit tabs |
| Bookmark star | Tap | Atomically add/remove current page |
| Bookmarks | Tap | Expand/collapse private saved list |
| Bookmark page | Earlier/More | Show up to eight saved items |
| Dismiss | Escape | Close NuraLoumi control surface |

Tab targets are at least **48 logical pixels tall** and are outside the
scrolling content viewport. At 720–820 logical pixels wide the Cairo
browser menu uses three columns for actions; the address row spans the
full width. Action selection uses stable IDs rather than page titles.

The address field never accepts control characters or unbounded input and
does not overwrite a partially typed value during status refresh. All
URI authority/credential/port checks belong to the native engine validator.
The control protocol is private to the same UID; pages cannot invoke it.

The offline Rust browser fixture, software scenes and host WPE compilation
are NOT evidence of SL101 touch input, real compositor positioning, valid
graphics pixels, sandbox closure or a usable released browser.
