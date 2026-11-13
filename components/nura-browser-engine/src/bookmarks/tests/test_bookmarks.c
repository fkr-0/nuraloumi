/* SPDX-License-Identifier: MIT */
#define _POSIX_C_SOURCE 200809L
#include "../bookmarks.h"
#include <glib/gstdio.h>
#include <sys/stat.h>
#include <unistd.h>

static char *private_dir(void)
{
    GError *error = NULL;
    char *dir = g_dir_make_tmp("nura-bookmark-test-XXXXXX", &error);
    g_assert_no_error(error);
    g_assert_nonnull(dir);
    g_assert_cmpint(g_chmod(dir, 0700), ==, 0);
    return dir;
}

static void cleanup(const char *dir)
{
    g_autofree char *bookmarks = g_build_filename(dir, "bookmarks.v1", NULL);
    g_unlink(bookmarks);
    g_assert_cmpint(g_rmdir(dir), ==, 0);
}

static NuraBookmarkStore *open_store(const char *dir)
{
    GError *error = NULL;
    NuraBookmarkStore *store = nurabrowser_bookmarks_open(dir, &error);
    g_assert_no_error(error);
    g_assert_nonnull(store);
    return store;
}

static void test_roundtrip(void)
{
    g_autofree char *dir = private_dir();
    NuraBookmarkStore *store = open_store(dir);
    GError *error = NULL;
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, 0);
    g_assert_true(nurabrowser_bookmarks_add(store, "https://example.org/a", "Example A", &error));
    g_assert_no_error(error);
    g_assert_true(nurabrowser_bookmarks_add(store, "https://example.org/b", "B \u2713", &error));
    g_assert_no_error(error);
    g_assert_true(nurabrowser_bookmarks_add(store, "https://example.org/a", "Updated", &error));
    g_assert_no_error(error);
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, 2);
    g_assert_cmpstr(nurabrowser_bookmarks_get(store, 0)->title, ==, "Updated");
    g_assert_true(nurabrowser_bookmarks_contains(store, "https://example.org/b"));
    nurabrowser_bookmarks_close(store);

    g_autofree char *path = g_build_filename(dir, "bookmarks.v1", NULL);
    struct stat st;
    g_assert_cmpint(g_lstat(path, &st), ==, 0);
    g_assert_true(S_ISREG(st.st_mode));
    g_assert_cmpuint(st.st_mode & 0777, ==, 0600);
    g_assert_cmpuint(st.st_nlink, ==, 1);

    store = open_store(dir);
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, 2);
    g_assert_cmpstr(nurabrowser_bookmarks_get(store, 1)->url, ==,
                    "https://example.org/b");
    g_assert_true(nurabrowser_bookmarks_remove(store, "https://example.org/a", &error));
    g_assert_no_error(error);
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, 1);
    g_assert_false(nurabrowser_bookmarks_contains(store, "https://example.org/a"));
    nurabrowser_bookmarks_close(store);
    store = open_store(dir);
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, 1);
    nurabrowser_bookmarks_close(store);
    cleanup(dir);
}

static void test_limits_and_rollback(void)
{
    g_autofree char *dir = private_dir();
    NuraBookmarkStore *store = open_store(dir);
    GError *error = NULL;
    g_assert_false(nurabrowser_bookmarks_add(store, "file:///etc/passwd", "bad", &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
    g_assert_false(nurabrowser_bookmarks_add(store, "https://example.org/", "bad\nline", &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, 0);
    g_clear_error(&error);
    for (guint i = 0; i < NURABROWSER_MAX_BOOKMARKS; i++) {
        g_autofree char *url = g_strdup_printf("https://example.org/item%u", i);
        g_assert_true(nurabrowser_bookmarks_add(store, url, "Example", &error));
        g_assert_no_error(error);
    }
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, NURABROWSER_MAX_BOOKMARKS);
    g_assert_false(nurabrowser_bookmarks_add(store, "https://example.org/overflow",
                                             "Overflow", &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_NO_SPACE);
    g_clear_error(&error);
    g_assert_cmpuint(nurabrowser_bookmarks_count(store), ==, NURABROWSER_MAX_BOOKMARKS);
    nurabrowser_bookmarks_close(store);
    cleanup(dir);
}

static void test_symlink_and_corrupt_file_refused(void)
{
    g_autofree char *dir = private_dir();
    g_autofree char *file = g_build_filename(dir, "bookmarks.v1", NULL);
    g_autofree char *target = g_build_filename(dir, "target", NULL);
    g_assert_true(g_file_set_contents(target, "unchanged", -1, NULL));
    g_assert_cmpint(symlink(target, file), ==, 0);
    GError *error = NULL;
    g_assert_null(nurabrowser_bookmarks_open(dir, &error));
    g_assert_nonnull(error);
    g_clear_error(&error);
    g_assert_cmpint(g_unlink(file), ==, 0);
    g_assert_true(g_file_set_contents(file, "{\"version\":1,\"entries\":[", -1, NULL));
    g_assert_cmpint(g_chmod(file, 0600), ==, 0);
    g_assert_null(nurabrowser_bookmarks_open(dir, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_DATA);
    g_clear_error(&error);
    g_assert_cmpint(g_unlink(file), ==, 0);
    g_assert_cmpint(g_unlink(target), ==, 0);
    cleanup(dir);
}

static void test_read_only_directory_is_not_created(void)
{
    g_autofree char *dir = private_dir();
    g_autofree char *missing = g_build_filename(dir, "missing", NULL);
    GError *error = NULL;
    g_assert_null(nurabrowser_bookmarks_open(missing, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_PERMISSION_DENIED);
    g_clear_error(&error);
    g_assert_false(g_file_test(missing, G_FILE_TEST_EXISTS));
    cleanup(dir);
}

int main(int argc, char **argv)
{
    g_test_init(&argc, &argv, NULL);
    g_test_add_func("/browser/bookmarks/roundtrip", test_roundtrip);
    g_test_add_func("/browser/bookmarks/limits", test_limits_and_rollback);
    g_test_add_func("/browser/bookmarks/unsafe-file", test_symlink_and_corrupt_file_refused);
    g_test_add_func("/browser/bookmarks/unsafe-directory", test_read_only_directory_is_not_created);
    return g_test_run();
}
