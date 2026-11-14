/* No direct KMS / privileged compositor access. Bounded persistent xdg
 * surface and per-buffer lifetime tied to Wayland release events. */
#include "wayland-stream.h"
#include "linux-dmabuf.h"
#include "xdg-shell-client-protocol.h"
#include <wayland-client.h>
#include <poll.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <errno.h>

#define MAX_STREAM_BUFS 4
struct stream_bo {
    struct sl101_stream *parent;
    struct wl_buffer *buffer;
    struct gbm_surface *surface;
    struct gbm_bo *bo;
    int inuse;
    unsigned frame;
};
struct sl101_stream {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct zwp_linux_dmabuf_v1 *dma;
    struct xdg_wm_base *wm;
    struct wl_surface *surface;
    struct xdg_surface *xdg;
    struct xdg_toplevel *top;
    struct wl_callback *callback;
    struct stream_bo slots[MAX_STREAM_BUFS];
    unsigned configured,closed,done,callbacks,releases,submitted;
};
static void global(void *data,struct wl_registry *r,uint32_t id,
                   const char *interface,uint32_t version){
    struct sl101_stream *s=data;
    if(!strcmp(interface,wl_compositor_interface.name)&&version>=4)
        s->compositor=wl_registry_bind(r,id,&wl_compositor_interface,4);
    else if(!strcmp(interface,zwp_linux_dmabuf_v1_interface.name)&&version>=3)
        s->dma=wl_registry_bind(r,id,&zwp_linux_dmabuf_v1_interface,
                                version>4?4:version);
    else if(!strcmp(interface,xdg_wm_base_interface.name)&&version>=1)
        s->wm=wl_registry_bind(r,id,&xdg_wm_base_interface,
                              version>3?3:version);
}
static void removed(void *data,struct wl_registry *r,uint32_t id){
    (void)data;(void)r;(void)id;
}
static const struct wl_registry_listener registry_listener={global,removed};
static void ping(void *data,struct xdg_wm_base *wm,uint32_t serial){
    (void)data;xdg_wm_base_pong(wm,serial);
}
static const struct xdg_wm_base_listener wm_listener={ping};
static void configure(void *data,struct xdg_surface *xdg,uint32_t serial){
    struct sl101_stream *s=data;
    xdg_surface_ack_configure(xdg,serial);
    s->configured=1;
}
static const struct xdg_surface_listener xdg_listener={configure};
static void top_configure(void *data,struct xdg_toplevel *top,int32_t w,int32_t h,
                          struct wl_array *states){
    (void)data;(void)top;(void)w;(void)h;(void)states;
}
static void top_close(void *data,struct xdg_toplevel *top){
    (void)top;((struct sl101_stream *)data)->closed=1;
}
static const struct xdg_toplevel_listener top_listener={
    .configure=top_configure,.close=top_close
};
static void frame_done(void *data,struct wl_callback *callback,uint32_t ms){
    struct sl101_stream *s=data;
    s->done=1;
    s->callbacks++;
    if(s->callbacks<=2||s->callbacks%25==0)
        printf("WAYLAND_FRAME seq=%u compositor_time_ms=%u\n",s->callbacks,ms);
    wl_callback_destroy(callback);
    s->callback=NULL;
}
static const struct wl_callback_listener frame_listener={frame_done};
static void released(void *data,struct wl_buffer *buffer){
    struct stream_bo *p=data;
    if(!p->inuse||p->buffer!=buffer)return;
    struct sl101_stream *s=p->parent;
    if(p->bo)gbm_surface_release_buffer(p->surface,p->bo);
    p->bo=NULL;
    wl_buffer_destroy(p->buffer);
    p->buffer=NULL;
    p->inuse=0;
    s->releases++;
    if(s->releases<=2||s->releases%25==0)
        printf("WAYLAND_RELEASE frame=%u total=%u\n",p->frame,s->releases);
}
static const struct wl_buffer_listener release_listener={released};
static int pump(struct sl101_stream *s,unsigned *flag,unsigned limit){
    for(unsigned i=0;i<limit;i++){
        if(flag&&*flag)return 1;
        if(s->closed)return 0;
        if(wl_display_dispatch_pending(s->display)<0)return 0;
        if(flag&&*flag)return 1;
        if(wl_display_flush(s->display)<0 && errno!=EAGAIN)return 0;
        struct pollfd p={wl_display_get_fd(s->display),POLLIN,0};
        int n=poll(&p,1,60);
        if(n>0&&(p.revents&POLLIN)){
            if(wl_display_dispatch(s->display)<0)return 0;
        }else if(n<0 && errno!=EINTR)return 0;
        if(p.revents&(POLLERR|POLLHUP|POLLNVAL))return 0;
    }
    return !flag||*flag;
}
struct sl101_stream *sl101_stream_open(void){
    struct sl101_stream *s=calloc(1,sizeof(*s));
    if(!s)return NULL;
    s->display=wl_display_connect(NULL);
    if(!s->display)goto fail;
    s->registry=wl_display_get_registry(s->display);
    wl_registry_add_listener(s->registry,&registry_listener,s);
    if(wl_display_roundtrip(s->display)<0||
       !s->compositor||!s->dma||!s->wm)goto fail;
    xdg_wm_base_add_listener(s->wm,&wm_listener,s);
    s->surface=wl_compositor_create_surface(s->compositor);
    s->xdg=xdg_wm_base_get_xdg_surface(s->wm,s->surface);
    xdg_surface_add_listener(s->xdg,&xdg_listener,s);
    s->top=xdg_surface_get_toplevel(s->xdg);
    xdg_toplevel_add_listener(s->top,&top_listener,s);
    xdg_toplevel_set_title(s->top,"SL101 moving VDE DMA-BUF qualification");
    xdg_toplevel_set_app_id(s->top,"sl101-private-vde-moving");
    wl_surface_commit(s->surface);
    if(!pump(s,&s->configured,30))goto fail;
    puts("WAYLAND_STREAM_READY persistent_xdg=1 version_dmabuf>=3");
    return s;
fail:
    sl101_stream_close(s);
    return NULL;
}
static struct stream_bo *free_slot(struct sl101_stream *s){
    for(unsigned i=0;i<MAX_STREAM_BUFS;i++)
        if(!s->slots[i].inuse)return &s->slots[i];
    return NULL;
}
int sl101_stream_submit(struct sl101_stream *s,struct gbm_surface *surface,
                        struct gbm_bo *bo,int fd,unsigned frame){
    if(!s||s->closed||!bo||fd<0)return 0;
    struct stream_bo *slot=free_slot(s);
    for(unsigned i=0;!slot&&i<20;i++){
        if(!pump(s,NULL,1))return 0;
        slot=free_slot(s);
    }
    if(!slot){puts("WAYLAND_BACKPRESSURE slots=full");return 0;}
    uint64_t mod=gbm_bo_get_modifier(bo);
    uint32_t fourcc=gbm_bo_get_format(bo);
    if(fourcc!=0x34325258U||mod!=0){
        puts("WAYLAND_INVALID_OUTPUT expected=XR24-LINEAR");return 0;
    }
    struct zwp_linux_buffer_params_v1 *params=
        zwp_linux_dmabuf_v1_create_params(s->dma);
    if(!params)return 0;
    zwp_linux_buffer_params_v1_add(params,fd,0,0,gbm_bo_get_stride(bo),
                                   (uint32_t)(mod>>32),(uint32_t)mod);
    /* Version >=3 supports immediate create, without a round-trip per frame.
     * The compositor may reject this synchronously for an unsupported format. */
    struct wl_buffer *buffer=zwp_linux_buffer_params_v1_create_immed(
        params,gbm_bo_get_width(bo),gbm_bo_get_height(bo),fourcc,0);
    zwp_linux_buffer_params_v1_destroy(params);
    if(!buffer)return 0;
    slot->parent=s;
    slot->surface=surface;
    slot->bo=bo;
    slot->buffer=buffer;
    slot->frame=frame;
    slot->inuse=1;
    wl_buffer_add_listener(buffer,&release_listener,slot);
    s->done=0;
    s->callback=wl_surface_frame(s->surface);
    wl_callback_add_listener(s->callback,&frame_listener,s);
    wl_surface_attach(s->surface,buffer,0,0);
    wl_surface_damage_buffer(s->surface,0,0,gbm_bo_get_width(bo),
                             gbm_bo_get_height(bo));
    wl_surface_commit(s->surface);
    s->submitted++;
    if(!pump(s,&s->done,35)){
        printf("WAYLAND_FRAME_TIMEOUT seq=%u\n",frame);
        return 0;
    }
    /* Never destroy the current buffer: a future replacement commit or
     * final detach must trigger wl_buffer.release before releasing its BO. */
    return 1;
}
int sl101_stream_close(struct sl101_stream *s){
    if(!s)return 1;
    int clean=1;
    if(s->surface){
        wl_surface_attach(s->surface,NULL,0,0);
        wl_surface_commit(s->surface);
    }
    unsigned expected=s->submitted;
    for(unsigned i=0;i<75&&s->releases<expected;i++){
        if(!pump(s,NULL,1)){clean=0;break;}
    }
    if(s->releases!=expected)clean=0;
    printf("WAYLAND_STREAM_RESULT submitted=%u callbacks=%u releases=%u "
           "outstanding=%u pass=%d\n",
           s->submitted,s->callbacks,s->releases,
           s->submitted-s->releases,clean);
    if(s->callback)wl_callback_destroy(s->callback);
    /* On failed release, DO NOT free a compositor-owned GBM front buffer.
     * The caller should treat this as a hard qualification failure.
     * On connection loss the compositor cannot keep referencing it; the
     * process exits after cleanup so GBM close handles remaining BOs. */
    for(unsigned i=0;i<MAX_STREAM_BUFS;i++){
        struct stream_bo *p=&s->slots[i];
        if(p->buffer)wl_buffer_destroy(p->buffer);
        if(p->inuse&&p->bo) {
            printf("WAYLAND_UNRELEASED frame=%u\n",p->frame);
            p->bo=NULL;
        }
    }
    if(s->top)xdg_toplevel_destroy(s->top);
    if(s->xdg)xdg_surface_destroy(s->xdg);
    if(s->surface)wl_surface_destroy(s->surface);
    if(s->wm)xdg_wm_base_destroy(s->wm);
    if(s->dma)zwp_linux_dmabuf_v1_destroy(s->dma);
    if(s->compositor)wl_compositor_destroy(s->compositor);
    if(s->registry)wl_registry_destroy(s->registry);
    if(s->display)wl_display_disconnect(s->display);
    free(s);
    return clean;
}
unsigned sl101_stream_releases(const struct sl101_stream *s){return s?s->releases:0;}
unsigned sl101_stream_callbacks(const struct sl101_stream *s){return s?s->callbacks:0;}
