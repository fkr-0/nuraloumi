/* SL101 private VDE I420 DMA-BUF -> Grate GLES2 RGB experiment.
 *
 * No Sway surface or DRM master. The only CPU pixel reads are independent
 * glReadPixels / GstVideoFrame oracle measurements, never a conversion path.
 * Keep every GstSample and GBM front-buffer alive until GPU completion.
 */
#define _GNU_SOURCE
#include <gst/gst.h>
#include <gst/app/gstappsink.h>
#include <gst/video/video.h>
#include <gst/allocators/gstdmabuf.h>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <gbm.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mman.h>
#include <errno.h>
#include "wayland-present.h"

#define W 854
#define H 480
#define DRM_R8 0x20203852U
#define DRM_XR24 0x34325258U
#define EGL_GBM 0x31D7

static int errors=0;
static int check(const char *stage) {
    GLenum e=glGetError();
    printf("GL_CHECK stage=%s error=0x%x\n",stage,e);
    if(e!=GL_NO_ERROR)errors++;
    return e==GL_NO_ERROR;
}
static GstPadProbeReturn video_meta_query(GstPad *pad,GstPadProbeInfo *info,void *arg){
    (void)pad;(void)arg;
    GstQuery *q=GST_PAD_PROBE_INFO_QUERY(info);
    if(q&&GST_QUERY_TYPE(q)==GST_QUERY_ALLOCATION){
        if(!gst_query_find_allocation_meta(q,GST_VIDEO_META_API_TYPE,NULL))
            gst_query_add_allocation_meta(q,GST_VIDEO_META_API_TYPE,NULL);
        printf("ALLOCATION_META offered=%d\n",
               gst_query_find_allocation_meta(q,GST_VIDEO_META_API_TYPE,NULL));
    }
    return GST_PAD_PROBE_OK;
}
static GLuint compile(GLenum type,const char *src) {
    GLuint s=glCreateShader(type);
    glShaderSource(s,1,&src,NULL);
    glCompileShader(s);
    GLint ok=0;glGetShaderiv(s,GL_COMPILE_STATUS,&ok);
    if(!ok){
        char log[1024]={0};glGetShaderInfoLog(s,sizeof(log)-1,NULL,log);
        printf("GLSL_COMPILE_FAILED type=%u log=%s\n",type,log);
        errors++;
        glDeleteShader(s);
        return 0;
    }
    return s;
}
static GLuint make_program(const char *fs) {
    const char *vs="attribute vec2 xy;varying vec2 uv;"
        "void main(){gl_Position=vec4(xy,0.,1.);uv=(xy+1.)*.5;}";
    GLuint v=compile(GL_VERTEX_SHADER,vs);
    GLuint f=compile(GL_FRAGMENT_SHADER,fs);
    if(!v||!f){if(v)glDeleteShader(v);if(f)glDeleteShader(f);return 0;}
    GLuint p=glCreateProgram();
    glAttachShader(p,v);glAttachShader(p,f);
    glBindAttribLocation(p,0,"xy");
    glLinkProgram(p);
    GLint ok=0;glGetProgramiv(p,GL_LINK_STATUS,&ok);
    if(!ok){printf("GLSL_LINK_FAILED\n");errors++;}
    glDeleteShader(v);glDeleteShader(f);
    return ok?p:0;
}
static void draw_nowait(GLuint p){
    static const GLfloat xy[]={-1.f,-1.f,3.f,-1.f,-1.f,3.f};
    glUseProgram(p);
    glVertexAttribPointer(0,2,GL_FLOAT,GL_FALSE,0,xy);
    glEnableVertexAttribArray(0);
    glViewport(0,0,W,H);
    glDrawArrays(GL_TRIANGLES,0,3);
}
static void draw(GLuint p){
    draw_nowait(p);
    glFinish();
}
static int pixels(const char *stage, const int expect[3],int tolerance){
    unsigned char p[4]={0};
    glReadPixels(W/2,H/2,1,1,GL_RGBA,GL_UNSIGNED_BYTE,p);
    int ok=glGetError()==GL_NO_ERROR;
    for(int i=0;i<3;i++) if(abs((int)p[i]-expect[i])>tolerance)ok=0;
    printf("PIXEL stage=%s expected=%d,%d,%d observed=%u,%u,%u,%u tolerance=%d pass=%d\n",
           stage,expect[0],expect[1],expect[2],p[0],p[1],p[2],p[3],tolerance,ok);
    return ok;
}
static int clip(int n){return n<0?0:n>255?255:n;}
static void pixel_reference(GstVideoFrame *frame,int x,int row,int rgb,int result[3]){
    int yuv[3]={0};
    for(int i=0;i<3;i++){
        const uint8_t *ptr=GST_VIDEO_FRAME_PLANE_DATA(frame,i);
        int stride=GST_VIDEO_FRAME_PLANE_STRIDE(frame,i);
        yuv[i]=ptr[(row>>(i!=0))*stride+(x>>(i!=0))];
    }
    if(!rgb){for(int i=0;i<3;i++)result[i]=yuv[i];return;}
    const int c=yuv[0]-16,d=yuv[1]-128,e=yuv[2]-128,l=298*c+128;
    result[0]=clip((l+459*e)>>8);
    result[1]=clip((l-55*d-136*e)>>8);
    result[2]=clip((l+541*d)>>8);
}
static int spatial_diagnostic(GstVideoFrame *frame,const char *stage,int rgb){
    static const int samples[][2]={{107,73},{213,120},{427,240},
                                   {640,359},{757,404}};
    int matched_tl=0,matched_flip=0;
    for(size_t i=0;i<sizeof(samples)/sizeof(samples[0]);i++){
        int x=samples[i][0],y=samples[i][1];
        int top[3]={0},flip[3]={0};
        uint8_t px[4]={0};
        pixel_reference(frame,x,y,rgb,top);
        pixel_reference(frame,x,H-1-y,rgb,flip);
        glReadPixels(x,y,1,1,GL_RGBA,GL_UNSIGNED_BYTE,px);
        GLenum err=glGetError();
        int top_err=0,flip_err=0;
        for(int c=0;c<3;c++){
            int a=abs((int)px[c]-top[c]),b=abs((int)px[c]-flip[c]);
            if(a>top_err)top_err=a;
            if(b>flip_err)flip_err=b;
        }
        int t=(top_err<=(rgb?15:4))&&err==GL_NO_ERROR;
        int f=(flip_err<=(rgb?15:4))&&err==GL_NO_ERROR;
        matched_tl+=t;matched_flip+=f;
        printf("SPATIAL stage=%s point=%zu xy=%d,%d GPU=%u,%u,%u top=%d,%d,%d flip=%d,%d,%d top_err=%d flip_err=%d top_pass=%d flip_pass=%d gl=0x%x\n",
               stage,i,x,y,px[0],px[1],px[2],
               top[0],top[1],top[2],flip[0],flip[1],flip[2],
               top_err,flip_err,t,f,err);
    }
    printf("SPATIAL_SUMMARY stage=%s top_pass=%d flip_pass=%d total=%zu\n",
           stage,matched_tl,matched_flip,sizeof(samples)/sizeof(samples[0]));
    return matched_tl>matched_flip?matched_tl:matched_flip;
}

int main(int argc,char **argv){
    setvbuf(stdout,NULL,_IONBF,0);
    int frame_index=1;
    int no_cpu_map=0;
    int present=0;
    int gpu_bench=0;
    int single_pack=0;
    int dual_pack=0;
    int dual_pack_gpu_copy=0;
    int rgb_two_pass=0;
    int driver_i420_four_pass=0;
    if(argc>=2){
        char *end=NULL;
        long n=strtol(argv[1],&end,10);
        if(!end||*end||n<1||n>120){
            fprintf(stderr,"usage: vde-gpu-probe [frame-index:1..120] [no-map]\n");
            return 2;
        }
        frame_index=(int)n;
    }
    if(argc>=3 && !strcmp(argv[2],"no-map"))no_cpu_map=1;
    else if(argc>=3 && !strcmp(argv[2],"cpu-map"))no_cpu_map=0;
    else if(argc>=3 && !strcmp(argv[2],"present"))present=1;
    else if(argc>=3){
        fprintf(stderr,"usage: vde-gpu-probe [frame-index:1..120] [no-map] [present]\n");
        return 2;
    }
    if(argc==4 && no_cpu_map && !strcmp(argv[3],"present"))present=1;
    else if(argc>3){
        if(argc==4 && no_cpu_map && !strcmp(argv[3],"gpu-bench24"))
            gpu_bench=1;
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"single-pack"))
            single_pack=1;
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"dual-pack"))
            dual_pack=1;
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"dual-pack-present")){
            dual_pack=1;present=1;
        }
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"dual-pack-bench24")){
            dual_pack=1;gpu_bench=1;
        }
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"four-pass")){
            dual_pack=1;rgb_two_pass=1;
        }
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"four-pass-present")){
            dual_pack=1;rgb_two_pass=1;present=1;
        }
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"four-pass-bench24")){
            dual_pack=1;rgb_two_pass=1;gpu_bench=1;
        }
        else if(argc==4 && !strcmp(argv[3],"driver-i420-4pass")){
            driver_i420_four_pass=1;
        }
        else if(argc==4 && !strcmp(argv[3],"driver-i420-4pass-present")){
            driver_i420_four_pass=1;present=1;
        }
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"driver-i420-4pass-bench24")){
            driver_i420_four_pass=1;gpu_bench=1;
        }
        else if(argc==4 && no_cpu_map && !strcmp(argv[3],"dual-pack-gpu-copy")){
            dual_pack=1;dual_pack_gpu_copy=1;
        }
        else{
            fprintf(stderr,"usage: vde-gpu-probe [frame-index:1..120] [no-map] [present|gpu-bench24]\n");
            return 2;
        }
    }
    if(argc>4){
        fprintf(stderr,"usage: vde-gpu-probe [frame-index:1..120] [no-map] [present|gpu-bench24]\n");
        return 2;
    }
    gst_init(&argc,&argv);
    int ret=3,fd=-1,plane_fd[3]={-1,-1,-1};
    GstElement *pipeline=NULL,*sink=NULL;
    GstSample *sample=NULL;
    GstVideoFrame oracle={0};int oracle_mapped=0;
    struct gbm_device *gbm=NULL;
    struct gbm_surface *pack_surface=NULL,*rgb_surface=NULL;
    struct gbm_bo *packed_bo=NULL,*rgb_bo=NULL;
    EGLDisplay display=EGL_NO_DISPLAY;
    EGLContext context=EGL_NO_CONTEXT;
    EGLSurface pack_egl=EGL_NO_SURFACE,rgb_egl=EGL_NO_SURFACE;
    EGLImageKHR plane_image[3]={EGL_NO_IMAGE_KHR,EGL_NO_IMAGE_KHR,EGL_NO_IMAGE_KHR};
    EGLImageKHR packed_image=EGL_NO_IMAGE_KHR;
    GLuint textures[4]={0},pack_programs[3]={0},rgb_programs[3]={0};
    GLuint single_pack_program=0,dual_pack_program=0,rgb_rg_program=0;
    GLuint driver_i420_program=0;
    PFNEGLDESTROYIMAGEKHRPROC destroy_image=NULL;
    GError *error=NULL;
    const int clean_capture=getenv("SL101_CAPTURE_CLEAN") &&
        strcmp(getenv("SL101_CAPTURE_CLEAN"),"1")==0;
    const char *desc=clean_capture?
      "filesrc location=/home/user/tds3e1-main.mp4 ! qtdemux name=d "
      "d.video_0 ! queue ! h264parse ! "
      "v4l2slh264dec capture-stride-align=64 "
      "! video/x-raw(memory:DMABuf),format=DMA_DRM,drm-format=YU12 "
      "! appsink name=probe max-buffers=2 drop=false sync=false":
      "filesrc location=/home/user/tds3e1-main.mp4 ! qtdemux name=d "
      "d.video_0 ! queue ! h264parse ! v4l2slh264dec "
      "! video/x-raw(memory:DMABuf),format=DMA_DRM,drm-format=YU12 "
      "! appsink name=probe max-buffers=2 drop=false sync=false";
    pipeline=gst_parse_launch(desc,&error);
    if(error||!pipeline){printf("PIPELINE_PARSE_FAILED %s\n",error?error->message:"unknown");goto out;}
    if(clean_capture){
        GstElement *decoder=gst_bin_get_by_name(GST_BIN(pipeline),"v4l2slh264dec0");
        if(!decoder){puts("BLOCKED clean-capture-decoder-missing");goto out;}
        guint requested=0;
        g_object_get(decoder,"capture-stride-align",&requested,NULL);
        gst_object_unref(decoder);
        const char *expected_dir=getenv("SL101_PRIVATE_GST_DIR");
        GstPlugin *plugin=gst_registry_find_plugin(gst_registry_get(),"v4l2codecs");
        const gchar *filename=plugin?gst_plugin_get_filename(plugin):NULL;
        int attested=expected_dir && filename &&
            g_str_has_prefix(filename,expected_dir) &&
            filename[strlen(expected_dir)]=='/';
        printf("CLEAN_CAPTURE_PLUGIN requested=%u filename=%s attested=%d\n",
               requested,filename?filename:"(none)",attested);
        if(plugin)gst_object_unref(plugin);
        if(requested!=64||!attested){
            puts("BLOCKED clean-capture-plugin-provenance");goto out;
        }
    }
    sink=gst_bin_get_by_name(GST_BIN(pipeline),"probe");
    if(!sink)goto out;
    GstPad *sinkpad=gst_element_get_static_pad(sink,"sink");
    gst_pad_add_probe(sinkpad,GST_PAD_PROBE_TYPE_QUERY_DOWNSTREAM,video_meta_query,NULL,NULL);
    gst_object_unref(sinkpad);
    if(gst_element_set_state(pipeline,GST_STATE_PLAYING)==GST_STATE_CHANGE_FAILURE)goto out;
    for(int i=1;i<=frame_index;i++){
        GstSample *next=gst_app_sink_try_pull_sample(GST_APP_SINK(sink),7*GST_SECOND);
        if(!next){printf("VDE_FRAME_FAILED index=%d\n",i);goto out;}
        if(sample)gst_sample_unref(sample);
        sample=next;
    }
    if(!sample){puts("VDE_FRAME_FAILED");goto out;}
    GstBuffer *buf=gst_sample_get_buffer(sample);
    printf("SELECTED_VDE_FRAME index=%d pts=%" G_GUINT64_FORMAT "\n",
        frame_index,GST_BUFFER_PTS(buf));
    GstCaps *caps=gst_sample_get_caps(sample);
    gchar *capsstr=gst_caps_to_string(caps);
    printf("VDE_ACTUAL_CAPS %s\n",capsstr);
    g_free(capsstr);
    GstVideoMeta *meta=gst_buffer_get_video_meta(buf);
    if(!meta||meta->n_planes!=3||gst_buffer_n_memory(buf)!=3){
        puts("BLOCKED no-three-plane-videometa");goto out;
    }
    GstVideoInfoDmaDrm info;
    gst_video_info_dma_drm_init(&info);
    if(!gst_video_info_dma_drm_from_caps(&info,caps)){
        puts("BLOCKED cannot-parse-dma-drm");goto out;
    }
    printf("VDE_DRM fourcc=0x%08x modifier=0x%016llx visible=%dx%d coded=%ux%u\n",
        info.drm_fourcc,(unsigned long long)info.drm_modifier,
        info.vinfo.width,info.vinfo.height,meta->width,meta->height);
    if(info.drm_fourcc!=0x32315559U||info.drm_modifier!=0||meta->width<W||meta->height<H){
        puts("BLOCKED unexpected-drm-layout");goto out;
    }
    if(getenv("SL101_EXPECT_CAPTURE_PAD64")&&
       strcmp(getenv("SL101_EXPECT_CAPTURE_PAD64"),"1")==0 &&
       (meta->width!=896 || meta->stride[0]!=896 || meta->stride[1]!=448 ||
        meta->stride[2]!=448)){
        puts("BLOCKED VDE capture-pitch64-not-applied");goto out;
    }
    int strides[3]={0};
    for(int i=0;i<3;i++){
        GstMemory *mem=gst_buffer_peek_memory(buf,i);
        if(!gst_is_dmabuf_memory(mem)){puts("BLOCKED not-DMABuf");goto out;}
        plane_fd[i]=gst_dmabuf_memory_get_fd(mem);
        strides[i]=meta->stride[i];
        printf("VDE_PLANE index=%d fd=%d stride=%d meta_offset=%zu size=%zu\n",
            i,plane_fd[i],strides[i],meta->offset[i],gst_memory_get_sizes(mem,NULL,NULL));
        if(plane_fd[i]<0||strides[i]<=0)goto out;
    }
    int ref[3]={0};
    if(no_cpu_map){
        /* Known frame fingerprints previously obtained with a real VDE
         * CPU diagnostic oracle for the pinned clip.  Never maps GstMemory
         * in this mode: checks only GPU output pixels, not input CPU data. */
        static const int expected[][4]={
            {1,16,127,127},{25,76,127,125},{100,168,136,119}
        };
        int matched=0;
        for(size_t i=0;i<sizeof(expected)/sizeof(expected[0]);i++)
            if(expected[i][0]==frame_index){
                for(int p=0;p<3;p++)ref[p]=expected[i][p+1];
                matched=1;break;
            }
        if(!matched){puts("BLOCKED no-map-unpinned-frame");goto out;}
        printf("VDE_ORACLE_MODE gpu-only/no-cpu-map frame=%d\n",frame_index);
    }else{
        /* Independent, diagnostic-only CPU oracle; never used to convert
         * images or populate a GPU texture. */
        if(!gst_video_frame_map(&oracle,&info.vinfo,buf,GST_MAP_READ)){
            puts("BLOCKED pixel-oracle-frame-map");goto out;
        }
        oracle_mapped=1;
        for(int i=0;i<3;i++){
            const uint8_t *ptr=GST_VIDEO_FRAME_PLANE_DATA(&oracle,i);
            int stride=GST_VIDEO_FRAME_PLANE_STRIDE(&oracle,i);
            ref[i]=ptr[(H/2>>(i!=0))*stride+(W/2>>(i!=0))];
        }
        puts("VDE_ORACLE_MODE diagnostic-cpu-map");
    }
    printf("VDE_ORACLE_CENTER YUV=%d,%d,%d\n",ref[0],ref[1],ref[2]);

    fd=open("/dev/dri/renderD128",O_RDWR|O_CLOEXEC);
    if(fd<0||!(gbm=gbm_create_device(fd))){puts("BLOCKED render-node");goto out;}
    PFNEGLGETPLATFORMDISPLAYEXTPROC get_display=(void*)eglGetProcAddress("eglGetPlatformDisplayEXT");
    PFNEGLCREATEPLATFORMWINDOWSURFACEEXTPROC window=(void*)eglGetProcAddress("eglCreatePlatformWindowSurfaceEXT");
    PFNEGLCREATEIMAGEKHRPROC create_image=(void*)eglGetProcAddress("eglCreateImageKHR");
    destroy_image=(void*)eglGetProcAddress("eglDestroyImageKHR");
    PFNGLEGLIMAGETARGETTEXTURE2DOESPROC image_target=(void*)eglGetProcAddress("glEGLImageTargetTexture2DOES");
    if(!get_display||!window||!create_image||!destroy_image||!image_target){
        puts("BLOCKED EGL dma-buf extension entrypoints");goto out;
    }
    display=get_display(EGL_GBM,gbm,NULL);
    EGLint maj=0,min=0,n=0,format=0;
    if(display==EGL_NO_DISPLAY||!eglInitialize(display,&maj,&min)){puts("BLOCKED EGL init");goto out;}
    const char *egl_ext=eglQueryString(display,EGL_EXTENSIONS);
    const char *gl_vendor=NULL,*gl_renderer=NULL;
    if(!egl_ext||!strstr(egl_ext,"EGL_EXT_image_dma_buf_import")){
        puts("BLOCKED EGL_EXT_image_dma_buf_import");goto out;
    }
    PFNEGLQUERYDMABUFFORMATSEXTPROC query_formats=
        (void *)eglGetProcAddress("eglQueryDmaBufFormatsEXT");
    if(query_formats && strstr(egl_ext,"EGL_EXT_image_dma_buf_import_modifiers")){
        EGLint num=0;
        if(query_formats(display,0,NULL,&num) && num>0 && num<256){
            EGLint *fmts=calloc((size_t)num,sizeof(*fmts));
            if(fmts){
                EGLint got=0;
                if(query_formats(display,num,fmts,&got)){
                    int r8=0,xr24=0;
                    for(int f=0;f<got;f++){
                        if((uint32_t)fmts[f]==DRM_R8)r8=1;
                        if((uint32_t)fmts[f]==DRM_XR24)xr24=1;
                    }
                    printf("EGL_SUPPORTED_FORMATS total=%d R8=%d XR24=%d\n",
                           got,r8,xr24);
                }
                free(fmts);
            }
        }
    }
    eglBindAPI(EGL_OPENGL_ES_API);
    const EGLint config_attrs[]={EGL_SURFACE_TYPE,EGL_WINDOW_BIT,EGL_RENDERABLE_TYPE,EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_NONE};
    EGLConfig config=NULL;
    if(!eglChooseConfig(display,config_attrs,&config,1,&n)||!n)goto out;
    eglGetConfigAttrib(display,config,EGL_NATIVE_VISUAL_ID,&format);
    printf("GBM_CONFIG fourcc=0x%08x egl=%d.%d\n",(unsigned)format,maj,min);
    if((unsigned)format!=DRM_XR24){puts("BLOCKED non-XR24-target");goto out;}
    pack_surface=gbm_surface_create(gbm,W,H,format,GBM_BO_USE_RENDERING);
    rgb_surface=gbm_surface_create(gbm,W,H,format,GBM_BO_USE_RENDERING);
    if(!pack_surface||!rgb_surface){puts("BLOCKED GBM surface");goto out;}
    pack_egl=window(display,config,pack_surface,NULL);
    rgb_egl=window(display,config,rgb_surface,NULL);
    const EGLint context_attrs[]={EGL_CONTEXT_CLIENT_VERSION,2,EGL_NONE};
    context=eglCreateContext(display,config,EGL_NO_CONTEXT,context_attrs);
    if(pack_egl==EGL_NO_SURFACE||rgb_egl==EGL_NO_SURFACE||context==EGL_NO_CONTEXT||
       !eglMakeCurrent(display,pack_egl,pack_egl,context)){
        printf("BLOCKED EGL context 0x%x\n",eglGetError());goto out;
    }
    gl_vendor=(const char*)glGetString(GL_VENDOR);
    gl_renderer=(const char*)glGetString(GL_RENDERER);
    printf("GL_RENDERER vendor=%s renderer=%s\n",gl_vendor?gl_vendor:"?",gl_renderer?gl_renderer:"?");
    if(!gl_vendor||strcmp(gl_vendor,"Grate")||!gl_renderer||!strstr(gl_renderer,"Tegra")){
        puts("BLOCKED wrong GLES driver");goto out;
    }
    glGenTextures(4,textures);
    for(int i=0;i<3;i++){
        const EGLint attrs[]={EGL_WIDTH,i?W/2:W,EGL_HEIGHT,i?H/2:H,
          EGL_LINUX_DRM_FOURCC_EXT,(EGLint)DRM_R8,EGL_DMA_BUF_PLANE0_FD_EXT,plane_fd[i],
          EGL_DMA_BUF_PLANE0_OFFSET_EXT,0,EGL_DMA_BUF_PLANE0_PITCH_EXT,strides[i],
          EGL_NONE};
        plane_image[i]=create_image(display,EGL_NO_CONTEXT,EGL_LINUX_DMA_BUF_EXT,NULL,attrs);
        printf("PLANE_IMPORT index=%d image_ok=%d egl_error=0x%x\n",
               i,plane_image[i]!=EGL_NO_IMAGE_KHR,eglGetError());
        if(plane_image[i]==EGL_NO_IMAGE_KHR){
            if(i==0 && (strides[i]%4)==0){
                /* Diagnostics only: determine whether the external fd is
                 * importable independently of unsupported R8. Reinterpreting
                 * the bytes as XR24 is NOT a pixel-correct Y plane. */
                const EGLint control_attrs[]={EGL_WIDTH,strides[i]/4,EGL_HEIGHT,H,
                    EGL_LINUX_DRM_FOURCC_EXT,(EGLint)DRM_XR24,
                    EGL_DMA_BUF_PLANE0_FD_EXT,plane_fd[i],
                    EGL_DMA_BUF_PLANE0_OFFSET_EXT,0,
                    EGL_DMA_BUF_PLANE0_PITCH_EXT,strides[i],EGL_NONE};
                EGLImageKHR control=create_image(display,EGL_NO_CONTEXT,
                                  EGL_LINUX_DMA_BUF_EXT,NULL,control_attrs);
                printf("CONTROL_XR24_VDE_FD import=%d egl_error=0x%x\n",
                       control!=EGL_NO_IMAGE_KHR,eglGetError());
                if(control!=EGL_NO_IMAGE_KHR)destroy_image(display,control);
            }
            goto out;
        }
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D,textures[i]);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,
                        driver_i420_four_pass?GL_LINEAR:GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,
                        driver_i420_four_pass?GL_LINEAR:GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
        image_target(GL_TEXTURE_2D,plane_image[i]);
        if(!check("plane-texture-target"))goto out;
    }
    if(driver_i420_four_pass){
        /* Grate's strict WebKit I420 TGSI matcher internally emits three
         * single-L8/R8 source passes into a temporary scratch texture and
         * one original-geometry resolve pass: four GPU raster passes.
         * This directly consumes the true VDE three-plane DMA-BUFs.
         * Crucially there is no CPU packing or intermediate application
         * GBM export/reimport step. The private 0088 Grate candidate is
         * required to qualify an EGL-imported R8 view in this legacy
         * multipass path. No production-player path is changed.
         */
        const char *frag=
          "precision mediump float;uniform mat4 m;uniform mat4 yuvm;"
          "uniform sampler2D sy;uniform sampler2D su;uniform sampler2D sv;"
          "varying vec2 uv;"
          "void main(){vec4 yuv1=vec4(texture2D(sy,uv).x,"
          "texture2D(su,uv).x,texture2D(sv,uv).x,1.0);"
          "vec4 q=vec4(dot(yuv1,yuvm[0]),dot(yuv1,yuvm[1]),"
          "dot(yuv1,yuvm[2]),1.0);gl_FragColor=m*q;}";
        driver_i420_program=make_program(frag);
        if(!driver_i420_program){
            puts("DIRECT_I420_REJECT reason=web-kit-shader-link");
            goto out;
        }
        const GLfloat k[3][4]={
            {1.164f,0.f,1.793f,-0.973051f},
            {1.164f,-0.213f,-0.533f,0.301427f},
            {1.164f,2.112f,0.f,-1.133176f}
        };
        GLfloat ym[16]={0};
        for(int i=0;i<3;i++)
            for(int j=0;j<4;j++)ym[i*4+j]=k[i][j];
        ym[15]=1.f;
        const GLfloat identity[16]={
            1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1
        };
        if(!eglMakeCurrent(display,rgb_egl,rgb_egl,context)){
            puts("BLOCKED i420-direct-make-current");goto out;
        }
        glUseProgram(driver_i420_program);
        glUniformMatrix4fv(glGetUniformLocation(driver_i420_program,"m"),
                           1,GL_FALSE,identity);
        glUniformMatrix4fv(glGetUniformLocation(driver_i420_program,"yuvm"),
                           1,GL_FALSE,ym);
        for(int i=0;i<3;i++){
            glActiveTexture(GL_TEXTURE0+i);
            glBindTexture(GL_TEXTURE_2D,textures[i]);
            glUniform1i(glGetUniformLocation(driver_i420_program,
                           i==0?"sy":i==1?"su":"sv"),i);
        }
        glActiveTexture(GL_TEXTURE0);
        glDisable(GL_BLEND);
        glDisable(GL_SCISSOR_TEST);
        glDisable(GL_DEPTH_TEST);
        glClearColor(0,0,0,1);glClear(GL_COLOR_BUFFER_BIT);
        draw(driver_i420_program);
        if(!check("direct-i420-driver-multipass"))goto out;
        int c=ref[0]-16,d=ref[1]-128,e=ref[2]-128,l=298*c+128;
        const int expected_rgb[3]={
            clip((l+459*e)>>8),clip((l-55*d-136*e)>>8),
            clip((l+541*d)>>8)
        };
        const int correct=pixels("direct-driver-I420-BT709-RGB",expected_rgb,15);
        int spatial_ok=1;
        if(oracle_mapped){
            const int spatial=spatial_diagnostic(&oracle,
                           "direct-driver-I420-BT709-RGB",1);
            spatial_ok=spatial>=4;
            printf("DIRECT_I420_SPATIAL_RESULT matched=%d required=4 pass=%d\n",
                   spatial,spatial_ok);
        }
        printf("DIRECT_I420_RESULT expected_internal_gpu_passes=4 rgb_pixels=%d spatial_ok=%d\n",
               correct,spatial_ok);
        if(!correct||!spatial_ok)goto out;
        if(!eglSwapBuffers(display,rgb_egl)){
            puts("BLOCKED direct-i420-rgb-swap");goto out;
        }
        rgb_bo=gbm_surface_lock_front_buffer(rgb_surface);
        int outfd=rgb_bo?gbm_bo_get_fd(rgb_bo):-1;
        int sway_ok=0;
        if(outfd>=0 && present)
            sway_ok=sl101_wayland_present_dmabuf(outfd,
                gbm_bo_get_format(rgb_bo),gbm_bo_get_modifier(rgb_bo),
                gbm_bo_get_stride(rgb_bo),W,H);
        if(outfd>=0)close(outfd);
        if(gpu_bench && outfd>=0){
            const gint64 begin=g_get_monotonic_time();
            unsigned frames=0;
            for(unsigned i=0;i<24;i++){
                glClearColor(0,0,0,1);glClear(GL_COLOR_BUFFER_BIT);
                draw_nowait(driver_i420_program);
                glFinish();
                if(!check("direct-i420-bench"))break;
                frames++;
            }
            const gint64 us=g_get_monotonic_time()-begin;
            printf("GPU_BENCH_RESULT passes=%u elapsed_ms=%.3f fps=%.3f "
                   "mode=repeat-same-vde-dmabuf gpu_internal_passes_per_frame=4 "
                   "gl_draw_calls_per_pass=1 includes_vde_decode=0 includes_wayland=0 "
                   "includes_new_dma_import=0 no_display_throttle=1\n",
                   frames,us/1000.0,
                   us>0?(double)frames*1000000.0/us:0);
            if(frames!=24)errors++;
        }
        printf("RESULT_DIRECT_I420 real_vde_dmabuf=1 rgb_pixels=%d "
               "sway_present=%d gpu_internal_passes=4 "
               "single_frame_gpu_transport_pass=%d sustained_video_qualified=0\n",
               correct,sway_ok,correct&&outfd>=0&&(!present||sway_ok));
        ret=correct&&outfd>=0&&(!present||sway_ok)&&!errors?0:7;
        goto out;
    }
    const char *pack_fs[3]={
        "precision mediump float;varying vec2 uv;uniform sampler2D t;"
        "void main(){float v=texture2D(t,uv).r;gl_FragColor=vec4(v,0.,0.,1.);}",
        "precision mediump float;varying vec2 uv;uniform sampler2D t;"
        "void main(){float v=texture2D(t,uv).r;gl_FragColor=vec4(0.,v,0.,1.);}",
        "precision mediump float;varying vec2 uv;uniform sampler2D t;"
        "void main(){float v=texture2D(t,uv).r;gl_FragColor=vec4(0.,0.,v,1.);}"
    };
    for(int i=0;i<3;i++){
        pack_programs[i]=make_program(pack_fs[i]);
        if(!pack_programs[i])goto out;
        glUseProgram(pack_programs[i]);
        glUniform1i(glGetUniformLocation(pack_programs[i],"t"),0);
    }
    glDisable(GL_DEPTH_TEST);
    glDisable(GL_SCISSOR_TEST);
    glDisable(GL_BLEND);
    glClearColor(0,0,0,1);
    glClear(GL_COLOR_BUFFER_BIT);
    if(dual_pack){
        /* Reduce three packed YUV passes to two without changing any GPU
         * format or cache behavior. This is an ORACLE candidate, not
         * activated for the normal player until hardware pixels pass. */
        const char *fs="precision mediump float;varying vec2 uv;"
            "uniform sampler2D ty;uniform sampler2D tu;"
            "void main(){float y=texture2D(ty,uv).r;"
            "float u=texture2D(tu,uv).r;"
            "gl_FragColor=vec4(y,u,0.,1.);}";
        dual_pack_program=make_program(fs);
        if(!dual_pack_program)goto out;
        /* Test opposite EXEC order: V first, then additive Y+U pair.
         * Previous YU->V order produced correct Y/U but zero V. */
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D,textures[2]);
        draw(pack_programs[2]);
        if(!check("dual-pack-V-first"))goto out;
        glEnable(GL_BLEND);
        glBlendEquation(GL_FUNC_ADD);
        glBlendFunc(GL_ONE,GL_ONE);
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D,textures[0]);
        glActiveTexture(GL_TEXTURE1);
        glBindTexture(GL_TEXTURE_2D,textures[1]);
        glUseProgram(dual_pack_program);
        glUniform1i(glGetUniformLocation(dual_pack_program,"ty"),0);
        glUniform1i(glGetUniformLocation(dual_pack_program,"tu"),1);
        draw(dual_pack_program);
        if(!check("dual-pack-YU-draw"))goto out;
        /* The next pipeline stage uses only sampler 0. Avoid leaking the
         * dual-pack sampler 1 binding into the YUV→RGB program state. */
        glActiveTexture(GL_TEXTURE1);
        glBindTexture(GL_TEXTURE_2D,0);
        glActiveTexture(GL_TEXTURE0);
    }else if(single_pack){
        const char *fs="precision mediump float;varying vec2 uv;"
            "uniform sampler2D ty;uniform sampler2D tu;uniform sampler2D tv;"
            "void main(){float y=texture2D(ty,uv).r;"
            "float u=texture2D(tu,uv).r;float v=texture2D(tv,uv).r;"
            "gl_FragColor=vec4(y,u,v,1.);}";
        single_pack_program=make_program(fs);
        if(!single_pack_program)goto out;
        for(int i=0;i<3;i++){
            glActiveTexture(GL_TEXTURE0+i);
            glBindTexture(GL_TEXTURE_2D,textures[i]);
        }
        glUseProgram(single_pack_program);
        glUniform1i(glGetUniformLocation(single_pack_program,"ty"),0);
        glUniform1i(glGetUniformLocation(single_pack_program,"tu"),1);
        glUniform1i(glGetUniformLocation(single_pack_program,"tv"),2);
        draw(single_pack_program);
        if(!check("single-pack-draw"))goto out;
    }else{
        glEnable(GL_BLEND);
        glBlendEquation(GL_FUNC_ADD);
        glBlendFunc(GL_ONE,GL_ONE);
        for(int i=0;i<3;i++){
            glActiveTexture(GL_TEXTURE0);
            glBindTexture(GL_TEXTURE_2D,textures[i]);
            draw(pack_programs[i]);
            if(!check("pack-draw"))goto out;
        }
    }
    glDisable(GL_BLEND);
    int pass_pack=pixels("gpu-packed-YUV",ref,6);
    if(single_pack)printf("SINGLE_PACK_RESULT pass=%d\n",pass_pack);
    if(dual_pack)printf("DUAL_PACK_RESULT pass=%d\n",pass_pack);
    if(oracle_mapped)spatial_diagnostic(&oracle,"gpu-packed-YUV",0);
    if(dual_pack_gpu_copy){
        /* Diagnostic: request an entirely GPU-side copy before switching
         * EGL surfaces. No exported/intermediate DMA-BUF, no CPU readback
         * feeds the RGB stage. The driver may decline this GLES2 copy. */
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D,textures[3]);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
        glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA,W,H,0,GL_RGBA,GL_UNSIGNED_BYTE,NULL);
        if(!check("dual-pack-gpu-copy-allocate"))goto out;
        glCopyTexSubImage2D(GL_TEXTURE_2D,0,0,0,0,0,W,H);
        if(!check("dual-pack-gpu-copy-transfer"))goto out;
        puts("PACKED_GPU_TRANSFER kind=glCopyTexSubImage2D");
    }else{
        if(!eglSwapBuffers(display,pack_egl)){puts("BLOCKED pack eglSwapBuffers");goto out;}
        packed_bo=gbm_surface_lock_front_buffer(pack_surface);
        if(!packed_bo){puts("BLOCKED packed frontbuffer");goto out;}
        int pfd=gbm_bo_get_fd(packed_bo);
        uint32_t pfmt=gbm_bo_get_format(packed_bo);
        uint64_t pmod=gbm_bo_get_modifier(packed_bo);
        int pstride=gbm_bo_get_stride(packed_bo);
        printf("PACKED_DMABUF fd_valid=%d fourcc=0x%x modifier=0x%016llx stride=%d\n",
               pfd>=0,pfmt,(unsigned long long)pmod,pstride);
        if(pfd<0||pfmt!=DRM_XR24||pmod!=0){if(pfd>=0)close(pfd);goto out;}
        if(getenv("SL101_DIAG_PACKED_BO_MAP")){
            /* Read-only diagnostic of the *exported* BO's center pixel.
             * This is never used to populate a texture or convert video.
             * Some dma-buf heaps do not expose CPU mmap: report ENOTSUP. */
            size_t len=(size_t)pstride*H;
            unsigned char *map=mmap(NULL,len,PROT_READ,MAP_SHARED,pfd,0);
            if(map!=MAP_FAILED){
                const size_t off=(size_t)(H/2)*pstride+(W/2)*4;
                printf("BO_PACKED_CPU_DIAG r=%u g=%u b=%u a=%u stride=%d offset=%zu source=exported-prime-fd\n",
                       map[off+2],map[off+1],map[off],map[off+3],pstride,off);
                munmap(map,len);
            }else{
                printf("BO_PACKED_CPU_DIAG mmap_failed errno=%d\n",errno);
            }
        }
        const EGLint pattrs[]={EGL_WIDTH,W,EGL_HEIGHT,H,EGL_LINUX_DRM_FOURCC_EXT,(EGLint)pfmt,
            EGL_DMA_BUF_PLANE0_FD_EXT,pfd,EGL_DMA_BUF_PLANE0_OFFSET_EXT,0,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,pstride,EGL_NONE};
        packed_image=create_image(display,EGL_NO_CONTEXT,EGL_LINUX_DMA_BUF_EXT,NULL,pattrs);
        printf("PACKED_GPU_REIMPORT ok=%d egl_error=0x%x\n",
               packed_image!=EGL_NO_IMAGE_KHR,eglGetError());
        close(pfd);
        if(packed_image==EGL_NO_IMAGE_KHR)goto out;
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D,textures[3]);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
        image_target(GL_TEXTURE_2D,packed_image);
        if(!check("packed-texture-target"))goto out;
    }
    if(!eglMakeCurrent(display,rgb_egl,rgb_egl,context)){puts("BLOCKED RGB makecurrent");goto out;}
    if(dual_pack){
        /* Isolate whether the two-TEX source bytes survived GBM export and
         * EGL reimport, independently of the later fused FP20 DP3. */
        const char *copy_fs=
          "precision mediump float;varying vec2 uv;uniform sampler2D t;"
          "void main(){gl_FragColor=texture2D(t,uv);}";
        GLuint copy_program=make_program(copy_fs);
        if(!copy_program)goto out;
        glDisable(GL_BLEND);glClearColor(0,0,0,1);glClear(GL_COLOR_BUFFER_BIT);
        glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,textures[3]);
        glUseProgram(copy_program);
        glUniform1i(glGetUniformLocation(copy_program,"t"),0);
        draw(copy_program);
        printf("DUAL_PACK_TRANSFER_COPY pass=%d\n",
               pixels("dual-pack-transfer-copy",ref,6));
        glDeleteProgram(copy_program);
    }
    const char *rgb_fs[3]={
        "precision mediump float;varying vec2 uv;uniform sampler2D t;uniform vec4 k;"
        "void main(){vec3 s=texture2D(t,uv).rgb;float c=dot(s,k.rgb)+k.a;"
        "gl_FragColor=vec4(c,0.,0.,1.);}",
        "precision mediump float;varying vec2 uv;uniform sampler2D t;uniform vec4 k;"
        "void main(){vec3 s=texture2D(t,uv).rgb;float c=dot(s,k.rgb)+k.a;"
        "gl_FragColor=vec4(0.,c,0.,1.);}",
        "precision mediump float;varying vec2 uv;uniform sampler2D t;uniform vec4 k;"
        "void main(){vec3 s=texture2D(t,uv).rgb;float c=dot(s,k.rgb)+k.a;"
        "gl_FragColor=vec4(0.,0.,c,1.);}"
    };
    const GLfloat k[3][4]={
       {1.164f,0.f,1.793f,-0.973051f},
       {1.164f,-0.213f,-0.533f,0.301427f},
       {1.164f,2.112f,0.f,-1.133176f}
    };
    /* Candidate: vectorize two RGB channels as a sparse 2x3 matrix,
     * avoiding two DP3 lowering scratch temporaries. The remaining blue
     * channel is generated by the existing single-channel shader.
     * Preserve full RGBA additive blending (partial masks are broken).
     * No live player or default shader is changed by this experiment. */
    if(rgb_two_pass){
        const char *fs=
            "precision mediump float;varying vec2 uv;"
            "uniform sampler2D t0;"
            "uniform vec4 kr;uniform vec4 kg;"
            "void main(){vec3 s=texture2D(t0,uv).rgb;"
            "vec2 v=s.r*vec2(kr.r,kg.r)+s.g*vec2(kr.g,kg.g)"
            "+s.b*vec2(kr.b,kg.b)+vec2(kr.a,kg.a);"
            "gl_FragColor=vec4(v,0.,1.);}";
        rgb_rg_program=make_program(fs);
        if(!rgb_rg_program){puts("FOUR_PASS_REJECT reason=vector-rg-shader-compile");goto out;}
    }
    glDisable(GL_BLEND);
    glClearColor(0,0,0,1);
    glClear(GL_COLOR_BUFFER_BIT);
    glEnable(GL_BLEND);
    glBlendEquation(GL_FUNC_ADD);
    glBlendFunc(GL_ONE,GL_ONE);
    for(int i=0;i<3;i++){
        rgb_programs[i]=make_program(rgb_fs[i]);
        if(!rgb_programs[i])goto out;
        glUseProgram(rgb_programs[i]);
        glUniform1i(glGetUniformLocation(rgb_programs[i],"t"),0);
        glUniform4fv(glGetUniformLocation(rgb_programs[i],"k"),1,k[i]);
    }
    if(rgb_two_pass){
        glActiveTexture(GL_TEXTURE0);
        glBindTexture(GL_TEXTURE_2D,textures[3]);
        glUseProgram(rgb_rg_program);
        glUniform1i(glGetUniformLocation(rgb_rg_program,"t0"),0);
        glUniform4fv(glGetUniformLocation(rgb_rg_program,"kr"),1,k[0]);
        glUniform4fv(glGetUniformLocation(rgb_rg_program,"kg"),1,k[1]);
        draw(rgb_rg_program);
        if(!check("rgb-rg-paired"))goto out;
        draw(rgb_programs[2]);
        if(!check("rgb-blue"))goto out;
    }else{
        for(int i=0;i<3;i++){
            glActiveTexture(GL_TEXTURE0);
            glBindTexture(GL_TEXTURE_2D,textures[3]);
            draw(rgb_programs[i]);
            if(!check("rgb-draw"))goto out;
        }
    }
    glDisable(GL_BLEND);
    int c=ref[0]-16,d=ref[1]-128,e=ref[2]-128,l=298*c+128;
    const int rgb_expected[3]={
        clip((l+459*e)>>8),clip((l-55*d-136*e)>>8),clip((l+541*d)>>8)
    };
    int pass_rgb=pixels("gpu-BT709-RGB",rgb_expected,12);
    if(rgb_two_pass)printf("FOUR_PASS_RESULT pack=%d rgb=%d\n",pass_pack,pass_rgb);
    if(oracle_mapped)spatial_diagnostic(&oracle,"gpu-BT709-RGB",1);
    if(!eglSwapBuffers(display,rgb_egl)){puts("BLOCKED RGB swap");goto out;}
    rgb_bo=gbm_surface_lock_front_buffer(rgb_surface);
    int outfd=rgb_bo?gbm_bo_get_fd(rgb_bo):-1;
    printf("OUTPUT_DMABUF fd_valid=%d fourcc=0x%x modifier=0x%016llx\n",
           outfd>=0,rgb_bo?gbm_bo_get_format(rgb_bo):0,
           (unsigned long long)(rgb_bo?gbm_bo_get_modifier(rgb_bo):~0ULL));
    int passed_sway=0;
    if(present && pass_pack && pass_rgb && outfd>=0)
        passed_sway=sl101_wayland_present_dmabuf(outfd,
                     gbm_bo_get_format(rgb_bo),gbm_bo_get_modifier(rgb_bo),
                     gbm_bo_get_stride(rgb_bo),W,H);
    if(outfd>=0)close(outfd);
    if(gpu_bench && pass_pack && pass_rgb && outfd>=0){
        /* No clock/VDE throttling. Re-render 24 full-size GPU pack+matrix
         * workloads using the same pinned decoded DMA-BUF input.
         * RGB samples the initially exported packed BO each cycle,
         * not the newly rendered pack buffer. This measures GPU draw
         * workload, NOT full decode/import/render/present video FPS. */
        const gint64 begin=g_get_monotonic_time();
        unsigned frames=0;
        for(unsigned i=0;i<24;i++){
            if(!eglMakeCurrent(display,pack_egl,pack_egl,context))break;
            glDisable(GL_BLEND);
            glClearColor(0,0,0,1);
            glClear(GL_COLOR_BUFFER_BIT);
            if(dual_pack) {
                glActiveTexture(GL_TEXTURE0);
                glBindTexture(GL_TEXTURE_2D,textures[2]);
                draw_nowait(pack_programs[2]);
                glEnable(GL_BLEND);
                glBlendEquation(GL_FUNC_ADD);
                glBlendFunc(GL_ONE,GL_ONE);
                glActiveTexture(GL_TEXTURE0);
                glBindTexture(GL_TEXTURE_2D,textures[0]);
                glActiveTexture(GL_TEXTURE1);
                glBindTexture(GL_TEXTURE_2D,textures[1]);
                glUseProgram(dual_pack_program);
                draw_nowait(dual_pack_program);
                glActiveTexture(GL_TEXTURE1);
                glBindTexture(GL_TEXTURE_2D,0);
            } else {
                glEnable(GL_BLEND);
                glBlendEquation(GL_FUNC_ADD);
                glBlendFunc(GL_ONE,GL_ONE);
                for(int p=0;p<3;p++){
                    glActiveTexture(GL_TEXTURE0);
                    glBindTexture(GL_TEXTURE_2D,textures[p]);
                    draw_nowait(pack_programs[p]);
                }
            }
            if(!check("bench-pack"))break;
            if(!eglMakeCurrent(display,rgb_egl,rgb_egl,context))break;
            glDisable(GL_BLEND);
            glClearColor(0,0,0,1);
            glClear(GL_COLOR_BUFFER_BIT);
            glEnable(GL_BLEND);
            glBlendEquation(GL_FUNC_ADD);
            glBlendFunc(GL_ONE,GL_ONE);
            glActiveTexture(GL_TEXTURE0);
            glBindTexture(GL_TEXTURE_2D,textures[3]);
            if(rgb_two_pass){
                draw_nowait(rgb_rg_program);
                draw_nowait(rgb_programs[2]);
            }else{
                for(int p=0;p<3;p++){
                    draw_nowait(rgb_programs[p]);
                }
            }
            glFinish();
            glDisable(GL_BLEND);
            if(!check("bench-rgb"))break;
            frames++;
        }
        const gint64 us=g_get_monotonic_time()-begin;
        printf("GPU_BENCH_RESULT passes=%u elapsed_ms=%.3f fps=%.3f mode=repeat-same-vde-dmabuf no_map=%d no_display_throttle=1 gpu_draw_calls_per_pass=%d gl_finish_per_pass=1 includes_new_dma_import=0 includes_vde_decode=0 includes_wayland=0 rgb_samples_initial_packed_bo=1 dual_pack=%d rgb_two_pass=%d\n",
               frames,us/1000.0,us>0?(double)frames*1000000.0/us:0,no_cpu_map,
               (dual_pack?2:3)+(rgb_two_pass?2:3),dual_pack,rgb_two_pass);
        if(frames!=24)errors++;
    }
    printf("RESULT real_vde_dmabuf=1 plane_egl_import=1 packed_yuv_pixels=%d bt709_rgb_pixels=%d export_rgb_dmabuf=%d sway_present=%d single_frame_gpu_transport_pass=%d sustained_video_qualified=0\n",
           pass_pack,pass_rgb,outfd>=0,passed_sway,
           pass_pack&&pass_rgb&&outfd>=0&&(!present||passed_sway));
    ret=(pass_pack&&pass_rgb&&outfd>=0&&errors==0&&
         (!present||passed_sway))?0:7;
out:
    if(display!=EGL_NO_DISPLAY&&eglGetCurrentContext()==context){
        glDeleteTextures(4,textures);
        for(int i=0;i<3;i++){
            if(pack_programs[i])glDeleteProgram(pack_programs[i]);
            if(rgb_programs[i])glDeleteProgram(rgb_programs[i]);
        }
        if(single_pack_program)glDeleteProgram(single_pack_program);
        if(dual_pack_program)glDeleteProgram(dual_pack_program);
        if(rgb_rg_program)glDeleteProgram(rgb_rg_program);
        if(driver_i420_program)glDeleteProgram(driver_i420_program);
    }
    if(display!=EGL_NO_DISPLAY&&destroy_image){
        if(packed_image!=EGL_NO_IMAGE_KHR)destroy_image(display,packed_image);
        for(int i=0;i<3;i++)
            if(plane_image[i]!=EGL_NO_IMAGE_KHR)destroy_image(display,plane_image[i]);
    }
    if(display!=EGL_NO_DISPLAY&&context!=EGL_NO_CONTEXT)
        eglMakeCurrent(display,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
    if(packed_bo)gbm_surface_release_buffer(pack_surface,packed_bo);
    if(rgb_bo)gbm_surface_release_buffer(rgb_surface,rgb_bo);
    if(display!=EGL_NO_DISPLAY){
        if(context!=EGL_NO_CONTEXT)eglDestroyContext(display,context);
        if(pack_egl!=EGL_NO_SURFACE)eglDestroySurface(display,pack_egl);
        if(rgb_egl!=EGL_NO_SURFACE)eglDestroySurface(display,rgb_egl);
        eglTerminate(display);
    }
    if(pack_surface)gbm_surface_destroy(pack_surface);
    if(rgb_surface)gbm_surface_destroy(rgb_surface);
    if(gbm)gbm_device_destroy(gbm);
    if(fd>=0)close(fd);
    if(oracle_mapped)gst_video_frame_unmap(&oracle);
    if(sample)gst_sample_unref(sample);
    if(pipeline)gst_element_set_state(pipeline,GST_STATE_NULL);
    if(sink)gst_object_unref(sink);
    if(pipeline)gst_object_unref(pipeline);
    if(error)g_error_free(error);
    return ret;
}
