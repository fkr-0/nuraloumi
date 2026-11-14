#include "video-bridge.h"
#include <pthread.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <math.h>

struct NuraVideo {
    pthread_mutex_t mutex;
    pthread_t worker;
    char path[4096];
    NuraVideoStatus status;
    NuraVideoControls control;
};

const char *nura_video_path(const NuraVideo *h){return h->path;}
static void *worker_main(void *argument) {
    NuraVideo *h=argument;
    int rc=nura_video_render_loop(h);
    NuraVideoStatus snapshot;
    nura_video_status(h,&snapshot);
    if(rc && snapshot.state!=-1)
        nura_video_error(h,"VDE/GPU stream terminated before EOS");
    return NULL;
}
NuraVideo *nura_video_open(const char *path,double volume,int muted){
    if(!path || path[0]!='/' || !isfinite(volume) ||
       volume<0.0 || volume>1.0 || strlen(path)>=4096)return NULL;
    NuraVideo *h=calloc(1,sizeof(*h));
    if(!h)return NULL;
    memcpy(h->path,path,strlen(path)+1);
    h->control.seek_ms=-1;
    h->control.volume=volume;
    h->control.muted=!!muted;
    h->control.volume_dirty=1;
    if(pthread_mutex_init(&h->mutex,NULL)) {free(h);return NULL;}
    if(pthread_create(&h->worker,NULL,worker_main,h)){
        pthread_mutex_destroy(&h->mutex);
        free(h);
        return NULL;
    }
    return h;
}
int nura_video_command(NuraVideo *h,int op,int64_t arg){
    if(!h)return 0;
    int ok=1;
    pthread_mutex_lock(&h->mutex);
    switch(op){
      case 1: h->control.pause=0;break;
      case 2: h->control.pause=1;break;
      case 3: h->control.stop=1;break;
      case 4:
        if(arg<0 || arg>INT64_MAX/1000000)ok=0;
        else h->control.seek_ms=arg;
        break;
      case 5:
        if(arg<0||arg>1000)ok=0;
        else {h->control.volume=arg/1000.0;h->control.volume_dirty=1;}
        break;
      case 6: h->control.muted=!!arg;h->control.volume_dirty=1;break;
      default: ok=0;break;
    }
    pthread_mutex_unlock(&h->mutex);
    return ok;
}
int nura_video_status(NuraVideo *h,NuraVideoStatus *out){
    if(!h||!out)return 0;
    pthread_mutex_lock(&h->mutex);
    *out=h->status;
    pthread_mutex_unlock(&h->mutex);
    return 1;
}
NuraVideoControls nura_video_controls(NuraVideo *h){
    NuraVideoControls c={0};
    pthread_mutex_lock(&h->mutex);
    c=h->control;
    h->control.seek_ms=-1;
    h->control.volume_dirty=0;
    pthread_mutex_unlock(&h->mutex);
    return c;
}
void nura_video_publish(NuraVideo *h,int state,uint64_t pts,
                        uint64_t duration,uint64_t frames){
    pthread_mutex_lock(&h->mutex);
    if(h->status.state!=-1){
        h->status.state=state;
        h->status.position_ms=pts;
        h->status.duration_ms=duration;
        h->status.frames=frames;
    }
    pthread_mutex_unlock(&h->mutex);
}
void nura_video_error(NuraVideo *h,const char *message){
    pthread_mutex_lock(&h->mutex);
    h->status.state=-1;
    snprintf(h->status.error,sizeof(h->status.error),"%s",
             message ? message : "unknown video error");
    pthread_mutex_unlock(&h->mutex);
}
void nura_video_close(NuraVideo *h){
    if(!h)return;
    nura_video_command(h,3,0);
    pthread_join(h->worker,NULL);
    pthread_mutex_destroy(&h->mutex);
    free(h);
}
