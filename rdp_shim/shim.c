/**
 * rdp_shim/shim.c
 *
 * Thin C wrapper around FreeRDP's public API that presents a simplified,
 * callback-driven interface for the Rust FFI layer.
 *
 * Targets FreeRDP 3.x.
 *
 * Public functions (exported for Rust FFI):
 *   rdp_shim_connect       — open a connection
 *   rdp_shim_disconnect    — close cleanly
 *   rdp_shim_poll          — pump the event loop, returns false on disconnect
 *   rdp_shim_framebuffer   — get latest framebuffer pointer + dimensions
 *   rdp_shim_send_keyboard — send a keyboard event
 *   rdp_shim_send_mouse    — send a mouse event
 *   rdp_shim_resize        — resize the desktop
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <freerdp/freerdp.h>
#include <freerdp/gdi/gdi.h>
#include <freerdp/channels/channels.h>
#include <freerdp/client/cmdline.h>
#include <freerdp/constants.h>
#include <freerdp/settings.h>
#include <freerdp/input.h>
#include <winpr/wlog.h>

/* ------------------------------------------------------------------ */
/* Internal context  —  attached to the FreeRDP instance via a custom  */
 /* field so callback closures can find our state.                       */
 /* ------------------------------------------------------------------ */

typedef struct {
    /* The FreeRDP instance. */
    freerdp *instance;

    /* Latest framebuffer copy (BGRX 32-bit, width*height*4 bytes).
         * NULL until the first BeginPaint/EndPaint pair fires.             */
    uint8_t *fb;
    uint32_t fb_width;
    uint32_t fb_height;
    uint32_t fb_pitch;   /* bytes per row (may be > width*4 for alignment) */

    /* Connection result. */
    bool connected;
    bool disconnected;
    char error[256];
} RdpContext;

/* Per-instance magic pointer — FreeRDP allows one user-defined pointer
 * via instance->Context. We stash our RdpContext* there.                  */
#define RD_CONTEXT(_inst) ((RdpContext *)((_inst)->Context))

/* ------------------------------------------------------------------ */
/* GDI callbacks                                                        */
/* ------------------------------------------------------------------ */

static BOOL rdp_begin_paint(freerdp *inst)
{
    RdpContext *ctx = RD_CONTEXT(inst);
    if (!ctx) return FALSE;
    /* GDI prepares a buffer; we'll copy it in rdp_end_paint. */
    return gdi_init_ex(inst, inst->settings->ColorDepth, inst->settings->DesktopWidth,
                       inst->settings->DesktopHeight, NULL);
}

static BOOL rdp_end_paint(freerdp *inst)
{
    RdpContext *ctx = RD_CONTEXT(inst);
    if (!ctx) return FALSE;

    /* Get the GDI framebuffer. */
    rdpGdi *gdi = inst->gdi;
    if (!gdi || !gdi->primary || !gdi->primary->hdc || !gdi->primary->hdc->data)
        return TRUE; /* nothing drawn yet — not an error */

    uint32_t w = inst->settings->DesktopWidth;
    uint32_t h = inst->settings->DesktopHeight;
    uint32_t pitch = gdi->primary->hdc->pitch;
    uint32_t bpp = (inst->settings->ColorDepth + 7) / 8; /* bytes per pixel */
    uint32_t src_row_bytes = w * bpp;

    /* Allocate or reallocate the framebuffer copy. */
    uint32_t need = w * h * 4;
    if (!ctx->fb || ctx->fb_width != w || ctx->fb_height != h) {
        uint8_t *new_fb = (uint8_t *)realloc(ctx->fb, need);
        if (!new_fb) return TRUE; /* OOM — skip this frame */
        ctx->fb = new_fb;
        ctx->fb_width = w;
        ctx->fb_height = h;
        ctx->fb_pitch = pitch;
    }

    /* Copy and convert BGRX -> RGBA (byte-swap R and B).
         * FreeRDP GDI typically gives BGRX (bottom-up in some configs, but
         * top-down for GDI). We assume top-down rows.                    */
    uint8_t *src = (uint8_t *)gdi->primary->hdc->data;
    uint8_t *dst = ctx->fb;
    for (uint32_t y = 0; y < h; y++) {
        uint8_t *row_src = src + y * pitch;
        uint8_t *row_dst = dst + y * w * 4;
        for (uint32_t x = 0; x < w; x++) {
            uint8_t b = row_src[x * bpp + 0];
            uint8_t g = row_src[x * bpp + 1];
            uint8_t r = row_src[x * bpp + 2];
            uint8_t a = 0xff;
            row_dst[x * 4 + 0] = r;
            row_dst[x * 4 + 1] = g;
            row_dst[x * 4 + 2] = b;
            row_dst[x * 4 + 3] = a;
        }
    }
    return TRUE;
}

/* ------------------------------------------------------------------ */
/* Public API                                                            */
/* ------------------------------------------------------------------ */

/** Open an RDP connection.
 *
 * Returns an opaque pointer on success, NULL on failure.
 * The pointer must be freed with rdp_shim_disconnect().                */
void *rdp_shim_connect(const char *host, uint16_t port,
                        const char *user, const char *password,
                        uint32_t width, uint32_t height)
{
    RdpContext *ctx = (RdpContext *)calloc(1, sizeof(RdpContext));
    if (!ctx) return NULL;

    /* Create FreeRDP instance. */
    freerdp *inst = freerdp_new();
    if (!inst) {
        free(ctx);
        return NULL;
    }
    inst->ContextSize = sizeof(rdpContext);
    if (!freerdp_context_new(inst)) {
        freerdp_free(inst);
        free(ctx);
        return NULL;
    }

    /* Stash our context pointer. */
    inst->Context = (rdpContext *)ctx;
    ctx->instance = inst;

    /* Configure settings. */
    rdpSettings *settings = inst->settings;
    settings->ServerHostname = _strdup(host);
    settings->ServerPort = port;
    settings->Username = _strdup(user ? user : "");
    settings->Password = _strdup(password ? password : "");
    settings->DesktopWidth = width > 0 ? width : 1280;
    settings->DesktopHeight = height > 0 ? height : 720;
    settings->ColorDepth = 32;
    settings->Fullscreen = FALSE;
    settings->GrabKeyboard = FALSE;
    settings->DisableCredentialsDelegation = FALSE;
    settings->AutoLogonEnabled = TRUE;
    settings->AuthenticationOnly = FALSE;
    settings->IgnoreCertificate = TRUE;
    settings->AuthenticationLevel = AUTH_LEVEL_NONE;

    /* Register GDI callbacks. */
    inst->Update->BeginPaint = rdp_begin_paint;
    inst->Update->EndPaint = rdp_end_paint;

    /* Connect. */
    if (!freerdp_connect(inst)) {
        snprintf(ctx->error, sizeof(ctx->error), "freerdp_connect failed");
        rdp_shim_disconnect(ctx);
        return NULL;
    }

    ctx->connected = TRUE;
    return (void *)ctx;
}

/** Close the connection and free all resources. */
void rdp_shim_disconnect(void *vp)
{
    if (!vp) return;
    RdpContext *ctx = (RdpContext *)vp;
    if (ctx->instance) {
        if (ctx->connected)
            freerdp_disconnect(ctx->instance);
        freerdp_free(ctx->instance);
    }
    free(ctx->fb);
    free(ctx);
}

/** Pump the FreeRDP event loop.
 *
 * Returns true while the session is still alive.
 * Returns false when the session has disconnected (caller should call
 * rdp_shim_disconnect and clean up).                                    */
bool rdp_shim_poll(void *vp)
{
    if (!vp) return false;
    RdpContext *ctx = (RdpContext *)vp;
    if (!ctx->instance || ctx->disconnected) return false;

    /* Pump the FreeRDP message loop. */
    if (!freerdp_check_event_handles(ctx->instance, 0, NULL, NULL, NULL)) {
        ctx->disconnected = TRUE;
        return false;
    }

    /* Handle channel events (clipboard, drive redirection, etc.). */
    freerdp_channels_check_event_handles(ctx->instance);

    return true;
}

/** Get a pointer to the latest RGBA framebuffer.
 *
 * Returns NULL if no frame has been received yet.
 * width and height are filled with the buffer dimensions.
 * The returned pointer is valid until the next rdp_shim_poll() call.    */
const uint8_t *rdp_shim_framebuffer(void *vp, uint32_t *width, uint32_t *height)
{
    if (!vp || !width || !height) return NULL;
    RdpContext *ctx = (RdpContext *)vp;
    if (!ctx->fb) return NULL;
    *width = ctx->fb_width;
    *height = ctx->fb_height;
    return (const uint8_t *)ctx->fb;
}

/** Send a keyboard event. */
void rdp_shim_send_keyboard(void *vp, uint16_t scancode, bool down)
{
    if (!vp) return;
    RdpContext *ctx = (RdpContext *)vp;
    if (!ctx->instance || !ctx->instance->input) return;
    freerdp_input_send_keyboard_event(ctx->instance->input,
                                      down ? KBD_FLAGS_DOWN : KBD_FLAGS_RELEASE,
                                      scancode);
}

/** Send a mouse event.
 *
 * flags: PTR_FLAGS_MOVE, PTR_FLAGS_BUTTON1, etc.
 * x, y: mouse position in desktop coordinates.                         */
void rdp_shim_send_mouse(void *vp, uint16_t flags, uint16_t x, uint16_t y)
{
    if (!vp) return;
    RdpContext *ctx = (RdpContext *)vp;
    if (!ctx->instance || !ctx->instance->input) return;
    freerdp_input_send_mouse_event(ctx->instance->input, flags, x, y);
}

/** Resize the remote desktop. */
void rdp_shim_resize(void *vp, uint32_t width, uint32_t height)
{
    if (!vp) return;
    RdpContext *ctx = (RdpContext *)vp;
    if (!ctx->instance || !ctx->instance->settings) return;

    ctx->instance->settings->DesktopWidth = width > 0 ? width : 1280;
    ctx->instance->settings->DesktopHeight = height > 0 ? height : 720;

    /* Request a desktop resize via the update channel. */
    freerdp_peer *peer = ctx->instance->context;
    /* For a client-side resize we notify the server of the new size
         * via the Deactivate/Activate reconnection sequence.
         * FreeRDP 3.x uses DeactivateRemoteDesktop.                    */
    #if defined(FREERDP_VERSION_MAJOR) && FREERDP_VERSION_MAJOR >= 3
        freerdp_deactivate(ctx->instance);
    #endif
}
