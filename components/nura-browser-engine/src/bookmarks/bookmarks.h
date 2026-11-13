/* SPDX-License-Identifier: MIT */
#ifndef NURABROWSER_BOOKMARKS_H
#define NURABROWSER_BOOKMARKS_H

#include <gio/gio.h>

#define NURABROWSER_MAX_BOOKMARKS 64u

typedef struct _NuraBookmarkStore NuraBookmarkStore;
typedef struct {
    const char *url;    /* Borrowed until next add/remove or store close. */
    const char *title;
} NuraBookmark;

/* The supplied XDG state directory must already exist, be owned by this UID
 * and have 0700 mode with no symlink components. No /tmp fallback. */
NuraBookmarkStore *nurabrowser_bookmarks_open(const char *state_dir, GError **error);
void nurabrowser_bookmarks_close(NuraBookmarkStore *store);
guint nurabrowser_bookmarks_count(const NuraBookmarkStore *store);
const NuraBookmark *nurabrowser_bookmarks_get(const NuraBookmarkStore *store, guint index);
gboolean nurabrowser_bookmarks_contains(const NuraBookmarkStore *store, const char *url);
gboolean nurabrowser_bookmarks_add(NuraBookmarkStore *store, const char *url,
                                   const char *title, GError **error);
gboolean nurabrowser_bookmarks_remove(NuraBookmarkStore *store, const char *url,
                                      GError **error);

#endif
