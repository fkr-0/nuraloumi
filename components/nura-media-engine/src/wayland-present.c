/* Isolated wl_buffer creation, bounded surface frame and release protocol.
 * Never changes Sway configuration or installs a video sink. */
#include "wayland-present.h"
#include <wayland-client.h>
#include "linux-dmabuf.h"
#include "xdg-shell-client-protocol.h"
#include <poll.h>
#include <stdio.h>
#include <string.h>

struct state {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct zwp_linux_dmabuf_v1 *dma;
    struct xdg_wm_base *wm;
    struct wl_surface *surface;
    struct xdg_surface *xdg;
    struct xdg_toplevel *top;
    struct zwp_linux_buffer_params_v1 *params;
    struct wl_buffer *buffer;
    struct wl_callback *frame;
    int configured,import_done,import_ok,frame_done,released,closed;
};
static void global(void *data,struct wl_registry *registry,
                   uint32_t name,const char *interface,uint32_t version){
    struct state *s=data;
    if(!strcmp(interface,wl_compositor_interface.name)&&version>=4)
        s->compositor=wl_registry_bind(registry,name,&wl_compositor_interface,4);
    else if(!strcmp(interface,zwp_linux_dmabuf_v1_interface.name)&&version>=3)
        s->dma=wl_registry_bind(registry,name,&zwp_linux_dmabuf_v1_interface,
                                version>4?4:version);
    else if(!strcmp(interface,xdg_wm_base_interface.name)&&version>=1)
        s->wm=wl_registry_bind(registry,name,&xdg_wm_base_interface,
                               version>3?3:version);
}
static void global_remove(void *data,struct wl_registry *r,uint32_t name){
    (void)data;(void)r;(void)name;
}
static const struct wl_registry_listener registry_listener={global,global_remove};
static void ping(void *data,struct xdg_wm_base *wm,uint32_t serial){
    (void)data;xdg_wm_base_pong(wm,serial);
}
static const struct xdg_wm_base_listener wm_listener={ping};
static void xdg_configure(void *data,struct xdg_surface *xdg,uint32_t serial){
    struct state *s=data;xdg_surface_ack_configure(xdg,serial);s->configured=1;
}
static const struct xdg_surface_listener xdg_listener={xdg_configure};
static void top_configure(void *data,struct xdg_toplevel *top,int32_t w,int32_t h,
                          struct wl_array *states){
    (void)data;(void)top;(void)w;(void)h;(void)states;
}
static void top_close(void *data,struct xdg_toplevel *top){
    (void)top;((struct state*)data)->closed=1;
}
static const struct xdg_toplevel_listener top_listener={
    .configure=top_configure,.close=top_close
};
static void buffer_created(void *data,struct zwp_linux_buffer_params_v1 *params,
                           struct wl_buffer *buffer){
    (void)params;struct state *s=data;s->buffer=buffer;s->import_done=1;s->import_ok=1;
}
static void buffer_failed(void *data,struct zwp_linux_buffer_params_v1 *params){
    (void)params;struct state *s=data;s->import_done=1;
}
static const struct zwp_linux_buffer_params_v1_listener params_listener={
    buffer_created,buffer_failed
};
static void frame_done(void *data,struct wl_callback *cb,uint32_t time){
    struct state *s=data;s->frame_done=1;
    printf("SWAY_FRAME_CALLBACK time_ms=%u\n",time);
    wl_callback_destroy(cb);s->frame=NULL;
}
static const struct wl_callback_listener frame_listener={frame_done};
static void released(void *data,struct wl_buffer *buf){
    (void)buf;((struct state*)data)->released=1;
    puts("SWAY_BUFFER_RELEASED event=release");
}
static const struct wl_buffer_listener buffer_listener={released};
static int pump(struct state *s,int *flag,unsigned cycles){
    for(unsigned i=0;i<cycles;i++){
        if(*flag)return 1;
        if(wl_display_dispatch_pending(s->display)<0)return 0;
        if(*flag)return 1;
        if(wl_display_flush(s->display)<0)return 0;
        struct pollfd pfd={wl_display_get_fd(s->display),POLLIN,0};
        int rc=poll(&pfd,1,150);
        if(rc>0&&(pfd.revents&POLLIN)){
            if(wl_display_dispatch(s->display)<0)return 0;
        }else if(rc<0)return 0;
    }
    return *flag;
}
int sl101_wayland_present_dmabuf(int fd,uint32_t fourcc,uint64_t modifier,
                                 int stride,int width,int height){
    struct state s={0};
    int pass=0;
    s.display=wl_display_connect(NULL);
    if(!s.display){puts("SWAY_PRESENT blocked=no-wayland-connection");return 0;}
    s.registry=wl_display_get_registry(s.display);
    wl_registry_add_listener(s.registry,&registry_listener,&s);
    if(wl_display_roundtrip(s.display)<0||
       !s.compositor||!s.dma||!s.wm){
        puts("SWAY_PRESENT blocked=missing-required-globals");
        goto cleanup;
    }
    xdg_wm_base_add_listener(s.wm,&wm_listener,&s);
    s.surface=wl_compositor_create_surface(s.compositor);
    s.xdg=xdg_wm_base_get_xdg_surface(s.wm,s.surface);
    xdg_surface_add_listener(s.xdg,&xdg_listener,&s);
    s.top=xdg_surface_get_toplevel(s.xdg);
    xdg_toplevel_add_listener(s.top,&top_listener,&s);
    xdg_toplevel_set_title(s.top,"SL101 VDE GPU private DMA-BUF proof");
    xdg_toplevel_set_app_id(s.top,"sl101-vde-gpu-qualify");
    wl_surface_commit(s.surface);
    if(!pump(&s,&s.configured,15)){
        puts("SWAY_PRESENT blocked=no-xdg-configure");
        goto cleanup;
    }
    s.params=zwp_linux_dmabuf_v1_create_params(s.dma);
    zwp_linux_buffer_params_v1_add(s.params,fd,0,0,stride,
                                   (uint32_t)(modifier>>32),(uint32_t)modifier);
    zwp_linux_buffer_params_v1_add_listener(s.params,&params_listener,&s);
    zwp_linux_buffer_params_v1_create(s.params,width,height,fourcc,0);
    if(!pump(&s,&s.import_done,15)||!s.import_ok){
        puts("SWAY_PRESENT blocked=linux-dmabuf-import");
        goto cleanup;
    }
    wl_buffer_add_listener(s.buffer,&buffer_listener,&s);
    printf("SWAY_PRESENT_BUFFER accepted=1 fourcc=0x%08x modifier=0x%016llx\n",
        fourcc,(unsigned long long)modifier);
    s.frame=wl_surface_frame(s.surface);
    wl_callback_add_listener(s.frame,&frame_listener,&s);
    wl_surface_attach(s.surface,s.buffer,0,0);
    wl_surface_damage_buffer(s.surface,0,0,width,height);
    wl_surface_commit(s.surface);
    int framed=pump(&s,&s.frame_done,15);
    printf("SWAY_PRESENT_FRAME callback=%d closed=%d\n",framed,s.closed);
    if(!framed||s.closed)goto cleanup;
    wl_surface_attach(s.surface,NULL,0,0);
    wl_surface_commit(s.surface);
    int released=pump(&s,&s.released,15);
    printf("SWAY_PRESENT_RELEASE received=%d\n",released);
    pass=framed&&released;
cleanup:
    if(s.frame)wl_callback_destroy(s.frame);
    if(s.buffer)wl_buffer_destroy(s.buffer);
    if(s.params)zwp_linux_buffer_params_v1_destroy(s.params);
    if(s.top)xdg_toplevel_destroy(s.top);
    if(s.xdg)xdg_surface_destroy(s.xdg);
    if(s.surface)wl_surface_destroy(s.surface);
    if(s.wm)xdg_wm_base_destroy(s.wm);
    if(s.dma)zwp_linux_dmabuf_v1_destroy(s.dma);
    if(s.compositor)wl_compositor_destroy(s.compositor);
    if(s.registry)wl_registry_destroy(s.registry);
    if(s.display)wl_display_disconnect(s.display);
    printf("SWAY_PRESENT_RESULT buffer_created=%d frame_callback=%d released=%d pass=%d\n",
           s.import_ok,s.frame_done,s.released,pass);
    return pass;
}
