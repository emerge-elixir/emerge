/* SPDX-License-Identifier: Apache-2.0
 * Narrow, owned-event ABI; no Rust callbacks cross the C boundary. */
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include "libinput.h"
#include "quirks.h"

struct emerge_input_event {
    uint64_t device;
    uint32_t kind, code, state;
    int32_t slot;
    double x, y;
};

static int restricted_open(const char *path, int flags, void *data) {
    (void)data;
    /* Emerge exposes no LED/output API. Read-only input/clock ioctls preserve
     * the existing readable-node requirement instead of requiring write access. */
    int fd = open(path, (flags & ~O_ACCMODE) | O_RDONLY | O_CLOEXEC | O_NONBLOCK);
    return fd < 0 ? -errno : fd;
}
int emerge_input_test_open(const char *path) { return restricted_open(path, O_RDWR, NULL); }
static void restricted_close(int fd, void *data) { (void)data; close(fd); }
static const struct libinput_interface interface = { restricted_open, restricted_close };

struct libinput *emerge_input_create(void) {
    struct libinput *ctx = libinput_path_create_context(&interface, NULL);
    if (ctx) libinput_log_set_handler(ctx, NULL);
    return ctx;
}
static void input_log(struct libinput *ctx, enum libinput_log_priority priority, const char *format, va_list args) {
    (void)ctx; (void)priority;
    fputs("drm_input libinput: ", stderr);
    vfprintf(stderr, format, args);
}
void emerge_input_logging(struct libinput *ctx, int enabled) {
    libinput_log_set_handler(ctx, enabled ? input_log : NULL);
    libinput_log_set_priority(ctx, LIBINPUT_LOG_PRIORITY_DEBUG);
}
void emerge_input_destroy(struct libinput *ctx) { libinput_unref(ctx); }
int emerge_input_fd(struct libinput *ctx) { return libinput_get_fd(ctx); }
int emerge_input_dispatch(struct libinput *ctx) { return libinput_dispatch(ctx); }
struct libinput_device *emerge_input_add(struct libinput *ctx, const char *path) {
    struct libinput_device *device = libinput_path_add_device(ctx, path);
    return device ? libinput_device_ref(device) : NULL;
}
void emerge_input_remove(struct libinput_device *device) {
    libinput_path_remove_device(device);
    libinput_device_unref(device);
}
int emerge_input_validate_quirks(struct libinput *ctx) {
    struct quirks_context *q = quirks_init_subsystem("emerge:bundled", NULL, NULL, ctx, QLOG_LIBINPUT_LOGGING);
    if (!q) return 0;
    quirks_context_unref(q);
    return 1;
}

int emerge_input_next(struct libinput *ctx, uint32_t width, uint32_t height, struct emerge_input_event *out) {
    struct libinput_event *event = libinput_get_event(ctx);
    if (!event) return 0;
    memset(out, 0, sizeof(*out));
    out->slot = -1;
    out->device = (uint64_t)(uintptr_t)libinput_event_get_device(event);
    enum libinput_event_type type = libinput_event_get_type(event);
    struct libinput_event_pointer *p;
    struct libinput_event_touch *t;
    switch (type) {
    case LIBINPUT_EVENT_DEVICE_REMOVED: out->kind = 1; break;
    case LIBINPUT_EVENT_KEYBOARD_KEY: {
        struct libinput_event_keyboard *key = libinput_event_get_keyboard_event(event);
        out->kind = 2;
        out->code = libinput_event_keyboard_get_key(key);
        out->state = libinput_event_keyboard_get_key_state(key);
        break;
    }
    case LIBINPUT_EVENT_POINTER_MOTION:
        p = libinput_event_get_pointer_event(event);
        out->kind = 3;
        out->x = libinput_event_pointer_get_dx(p);
        out->y = libinput_event_pointer_get_dy(p);
        break;
    case LIBINPUT_EVENT_POINTER_MOTION_ABSOLUTE:
        p = libinput_event_get_pointer_event(event);
        out->kind = 4;
        out->x = libinput_event_pointer_get_absolute_x_transformed(p, width);
        out->y = libinput_event_pointer_get_absolute_y_transformed(p, height);
        break;
    case LIBINPUT_EVENT_POINTER_BUTTON:
        p = libinput_event_get_pointer_event(event);
        out->kind = 5;
        out->code = libinput_event_pointer_get_button(p);
        out->state = libinput_event_pointer_get_button_state(p);
        break;
    case LIBINPUT_EVENT_POINTER_SCROLL_WHEEL:
    case LIBINPUT_EVENT_POINTER_SCROLL_FINGER:
    case LIBINPUT_EVENT_POINTER_SCROLL_CONTINUOUS:
        p = libinput_event_get_pointer_event(event);
        out->kind = type == LIBINPUT_EVENT_POINTER_SCROLL_WHEEL ? 6 : 7;
        if (libinput_event_pointer_has_axis(p, LIBINPUT_POINTER_AXIS_SCROLL_HORIZONTAL))
            out->x = out->kind == 6 ? libinput_event_pointer_get_scroll_value_v120(p, LIBINPUT_POINTER_AXIS_SCROLL_HORIZONTAL) / 120.0 : libinput_event_pointer_get_scroll_value(p, LIBINPUT_POINTER_AXIS_SCROLL_HORIZONTAL);
        if (libinput_event_pointer_has_axis(p, LIBINPUT_POINTER_AXIS_SCROLL_VERTICAL))
            out->y = out->kind == 6 ? libinput_event_pointer_get_scroll_value_v120(p, LIBINPUT_POINTER_AXIS_SCROLL_VERTICAL) / 120.0 : libinput_event_pointer_get_scroll_value(p, LIBINPUT_POINTER_AXIS_SCROLL_VERTICAL);
        break;
    case LIBINPUT_EVENT_TOUCH_DOWN:
    case LIBINPUT_EVENT_TOUCH_MOTION:
    case LIBINPUT_EVENT_TOUCH_UP:
    case LIBINPUT_EVENT_TOUCH_CANCEL:
        t = libinput_event_get_touch_event(event);
        out->kind = type == LIBINPUT_EVENT_TOUCH_DOWN ? 8 : type == LIBINPUT_EVENT_TOUCH_MOTION ? 9 : type == LIBINPUT_EVENT_TOUCH_UP ? 10 : 11;
        out->slot = libinput_event_touch_get_seat_slot(t);
        if (out->kind == 8 || out->kind == 9) {
            out->x = libinput_event_touch_get_x_transformed(t, width);
            out->y = libinput_event_touch_get_y_transformed(t, height);
        }
        break;
    case LIBINPUT_EVENT_TOUCH_FRAME: out->kind = 12; break;
    default: break; /* In particular, ignore legacy AXIS duplicates. */
    }
    libinput_event_destroy(event);
    return 1;
}
