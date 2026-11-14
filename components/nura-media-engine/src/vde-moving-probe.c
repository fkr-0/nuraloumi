/* SL101 private continuously changing VDE DMA-BUF → 4-pass Grate → Sway.
 * Uses the exact already-qualified shader/helpers from vde-gpu-probe.c.
 * No CPU YUV map/conversion, no DRM master and no system installation.
 */
#define main sl101_one_frame_reference_main
#include "vde-gpu-probe.c"
#undef main
#include "wayland-stream.h"
#include "video-bridge.h"

static int verify_plugin(GstElement *pipeline){
    GstElement *dec=gst_bin_get_by_name(GST_BIN(pipeline),"v4l2slh264dec0");
    if(!dec){puts("STREAM_GATE_BLOCKED decoder-not-found");return 0;}
    guint pad=0;g_object_get(dec,"capture-stride-align",&pad,NULL);
    gst_object_unref(dec);
    const char *dir=getenv("SL101_PRIVATE_GST_DIR");
    GstPlugin *plugin=gst_registry_find_plugin(gst_registry_get(),"v4l2codecs");
    const char *path=plugin?gst_plugin_get_filename(plugin):NULL;
    int ok=pad==64&&dir&&path&&g_str_has_prefix(path,dir)&&
           path[strlen(dir)]=='/';
    printf("STREAM_GST_PROVENANCE stride_align=%u private=%d\n",pad,ok);
    if(plugin)gst_object_unref(plugin);
    return ok;
}
static int check_bus(GstBus *bus){
    GstMessage *msg;
    int bad=0;
    while((msg=gst_bus_pop_filtered(bus,GST_MESSAGE_ERROR|GST_MESSAGE_EOS))){
        if(GST_MESSAGE_TYPE(msg)==GST_MESSAGE_ERROR){
            GError *e=NULL;gchar *dbg=NULL;
            gst_message_parse_error(msg,&e,&dbg);
            printf("STREAM_GST_ERROR source=%s reason=%s\n",
                    GST_OBJECT_NAME(msg->src),e?e->message:"unknown");
            g_clear_error(&e);g_free(dbg);bad++;
        }else puts("STREAM_EOS received=1");
        gst_message_unref(msg);
    }
    return bad;
}
static int validate(GstSample *sample,int strides[3],int fds[3],
                    unsigned frame){
    GstCaps *caps=gst_sample_get_caps(sample);
    GstBuffer *buf=gst_sample_get_buffer(sample);
    GstVideoMeta *meta=gst_buffer_get_video_meta(buf);
    GstVideoInfoDmaDrm info;gst_video_info_dma_drm_init(&info);
    if(!caps||!buf||!meta||meta->n_planes!=3||
       gst_buffer_n_memory(buf)!=3||
       !gst_video_info_dma_drm_from_caps(&info,caps)||
       info.drm_fourcc!=0x32315559U||info.drm_modifier!=0||
       info.vinfo.width!=W||info.vinfo.height!=H||
       meta->width!=896||meta->height<H||
       meta->stride[0]!=896||meta->stride[1]!=448||meta->stride[2]!=448||
       GST_VIDEO_INFO_FPS_N(&info.vinfo)!=24||
       GST_VIDEO_INFO_FPS_D(&info.vinfo)!=1||
       info.vinfo.colorimetry.matrix!=GST_VIDEO_COLOR_MATRIX_BT709)
    {
        printf("STREAM_LAYOUT_FAIL frame=%u\n",frame);
        return 0;
    }
    for(int p=0;p<3;p++){
        GstMemory *mem=gst_buffer_peek_memory(buf,p);
        if(!gst_is_dmabuf_memory(mem))return 0;
        fds[p]=gst_dmabuf_memory_get_fd(mem);
        strides[p]=meta->stride[p];
        if(fds[p]<0)return 0;
    }
    if(frame==1){
        char *text=gst_caps_to_string(caps);
        printf("STREAM_ACTUAL_CAPS %s\n",text);
        g_free(text);
        printf("STREAM_FIRST_VDE_PLANES stride=%d,%d,%d fd_count=3 cpu_map=0\n",
               strides[0],strides[1],strides[2]);
    }
    return 1;
}
static int import_frame(EGLDisplay ed,PFNEGLCREATEIMAGEKHRPROC create,
                        PFNGLEGLIMAGETARGETTEXTURE2DOESPROC image_target,
                        const int fds[3],const int strides[3],
                        GLuint tex[3],EGLImageKHR images[3]){
    glGenTextures(3,tex);
    for(int p=0;p<3;p++){
        const EGLint attrs[]={EGL_WIDTH,p?W/2:W,EGL_HEIGHT,p?H/2:H,
          EGL_LINUX_DRM_FOURCC_EXT,(EGLint)DRM_R8,
          EGL_DMA_BUF_PLANE0_FD_EXT,fds[p],
          EGL_DMA_BUF_PLANE0_OFFSET_EXT,0,
          EGL_DMA_BUF_PLANE0_PITCH_EXT,strides[p],EGL_NONE};
        images[p]=create(ed,EGL_NO_CONTEXT,EGL_LINUX_DMA_BUF_EXT,NULL,attrs);
        if(images[p]==EGL_NO_IMAGE_KHR)return 0;
        glActiveTexture(GL_TEXTURE0+p);
        glBindTexture(GL_TEXTURE_2D,tex[p]);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
        image_target(GL_TEXTURE_2D,images[p]);
    }
    return check("stream-import");
}
static void forget_frame(EGLDisplay ed,PFNEGLDESTROYIMAGEKHRPROC destroy,
                         GLuint tex[3],EGLImageKHR images[3]){
    glActiveTexture(GL_TEXTURE0);
    for(int i=0;i<3;i++){
        glActiveTexture(GL_TEXTURE0+i);
        glBindTexture(GL_TEXTURE_2D,0);
    }
    glActiveTexture(GL_TEXTURE0);
    glDeleteTextures(3,tex);
    for(int i=0;i<3;i++)if(images[i]!=EGL_NO_IMAGE_KHR)
        destroy(ed,images[i]);
}
static uint64_t sample_hash(unsigned char center[3]){
    /* RGB readback is a diagnostic ONLY. Every frame still renders from
     * three hardware DMA-BUF planes; no CPU YUV mapping or RGB upload. */
    const int at[][2]={{107,73},{213,120},{427,240},{640,359},{757,404}};
    uint64_t h=1469598103934665603ULL;
    for(unsigned i=0;i<5;i++){
        unsigned char p[4]={0};
        glReadPixels(at[i][0],at[i][1],1,1,GL_RGBA,GL_UNSIGNED_BYTE,p);
        if(i==2)
            for(int c=0;c<3;c++)center[c]=p[c];
        for(int c=0;c<3;c++){h^=p[c];h*=1099511628211ULL;}
    }
    return h;
}
/* Runtime library only: the unique NuraLoumi playerd owns this thread.
 * Media location and controls come from the existing playerd socket.
 * No second player process, CLI, or independent audio session.
 */
int nura_video_render_loop(NuraVideo *handle){
    /* playerd redirects stdout to its own log; emit complete evidence lines
     * without waiting for the daemon's ten-minute idle timeout. */
    setvbuf(stdout,NULL,_IONBF,0);
    const int until_eos=1;
    const unsigned target=24U*3600U*10U; /* bounded ten-hour session */
    const int audio=1;
    int interrupted=0,failed=0;
    unsigned idle_ticks=0;
    uint64_t duration_ms=0;
    int ret=7,devfd=-1;
    struct gbm_device *gbm=NULL;
    struct gbm_surface *surface=NULL;
    struct sl101_stream *stream=NULL;
    EGLDisplay ed=EGL_NO_DISPLAY;
    EGLSurface es=EGL_NO_SURFACE;
    EGLContext ctx=EGL_NO_CONTEXT;
    EGLConfig config=NULL;
    PFNEGLDESTROYIMAGEKHRPROC destroy=NULL;
    GstElement *pipeline=NULL,*sink=NULL,*amplifier=NULL;
    GstBus *bus=NULL;
    GLuint program=0;
    unsigned count=0,changed=0,pts_bad=0,pts_gap=0,checks=0;
    unsigned color_oracles=0,color_failures=0;
    int video_eos=0,pipeline_eos=0;
    GstClockTime firstpts=GST_CLOCK_TIME_NONE,lastpts=GST_CLOCK_TIME_NONE;
    uint64_t held_position_ms=0;
    gint64 first_us=0,last_us=0;
    double gpu_total_ms=0,post_pts_lag_max_ms=0,post_pts_lag_sum_ms=0;
    uint64_t previous_hash=0;

    gst_init(NULL,NULL);
    const char *base=
      "filesrc name=media_source ! qtdemux name=d "
      "d.video_0 ! queue max-size-buffers=4 max-size-bytes=0 max-size-time=0 "
      "! h264parse ! v4l2slh264dec capture-stride-align=64 "
      "! video/x-raw(memory:DMABuf),format=DMA_DRM,drm-format=YU12 "
      "! appsink name=probe sync=true max-buffers=3 drop=false ";
    const char *audio_desc=
      "d.audio_0 ! queue ! aacparse ! avdec_aac ! audioconvert "
      "! audioresample ! volume name=amplifier "
      "! pulsesink name=audiosink sync=true";
    char *desc=g_strdup_printf("%s%s",base,audio?audio_desc:"");
    GError *err=NULL;
    pipeline=gst_parse_launch(desc,&err);g_free(desc);
    if(!pipeline||err){
        printf("STREAM_PARSE_FAIL reason=%s\n",err?err->message:"unknown");
        if(err)g_error_free(err);
        goto done;
    }
    /* Set the path through a typed GObject property, never interpolated
     * into gst_parse_launch syntax. Rust already canonicalizes it and checks
     * it is a real regular file. */
    GstElement *file=gst_bin_get_by_name(GST_BIN(pipeline),"media_source");
    if(!file){nura_video_error(handle,"missing GStreamer file source");goto done;}
    g_object_set(file,"location",nura_video_path(handle),NULL);
    gst_object_unref(file);
    if(!verify_plugin(pipeline)){
        nura_video_error(handle,"missing pinned VDE capture plugin");goto done;
    }
    sink=gst_bin_get_by_name(GST_BIN(pipeline),"probe");
    if(!sink)goto done;
    GstPad *pad=gst_element_get_static_pad(sink,"sink");
    gst_pad_add_probe(pad,GST_PAD_PROBE_TYPE_QUERY_DOWNSTREAM,
                      video_meta_query,NULL,NULL);
    gst_object_unref(pad);
    amplifier=gst_bin_get_by_name(GST_BIN(pipeline),"amplifier");
    if(!amplifier){
        nura_video_error(handle,"missing unified audio volume control");
        goto done;
    }
    bus=gst_element_get_bus(pipeline);
    devfd=open("/dev/dri/renderD128",O_RDWR|O_CLOEXEC);
    if(devfd<0||!(gbm=gbm_create_device(devfd)))goto done;
    PFNEGLGETPLATFORMDISPLAYEXTPROC get_display=
        (void *)eglGetProcAddress("eglGetPlatformDisplayEXT");
    PFNEGLCREATEPLATFORMWINDOWSURFACEEXTPROC get_window=
        (void *)eglGetProcAddress("eglCreatePlatformWindowSurfaceEXT");
    PFNEGLCREATEIMAGEKHRPROC create=
        (void *)eglGetProcAddress("eglCreateImageKHR");
    destroy=(void *)eglGetProcAddress("eglDestroyImageKHR");
    PFNGLEGLIMAGETARGETTEXTURE2DOESPROC image_target=
        (void *)eglGetProcAddress("glEGLImageTargetTexture2DOES");
    if(!get_display||!get_window||!create||!destroy||!image_target)
        goto done;
    ed=get_display(EGL_GBM,gbm,NULL);
    EGLint major=0,minor=0,n=0,format=0;
    if(ed==EGL_NO_DISPLAY||!eglInitialize(ed,&major,&minor))goto done;
    eglBindAPI(EGL_OPENGL_ES_API);
    const EGLint attrs[]={EGL_SURFACE_TYPE,EGL_WINDOW_BIT,
         EGL_RENDERABLE_TYPE,EGL_OPENGL_ES2_BIT,
         EGL_RED_SIZE,8,EGL_GREEN_SIZE,8,EGL_BLUE_SIZE,8,EGL_NONE};
    if(!eglChooseConfig(ed,attrs,&config,1,&n)||n!=1)goto done;
    eglGetConfigAttrib(ed,config,EGL_NATIVE_VISUAL_ID,&format);
    if((unsigned)format!=DRM_XR24)goto done;
    surface=gbm_surface_create(gbm,W,H,format,GBM_BO_USE_RENDERING);
    if(!surface)goto done;
    es=get_window(ed,config,surface,NULL);
    const EGLint ctx_attrs[]={EGL_CONTEXT_CLIENT_VERSION,2,EGL_NONE};
    ctx=eglCreateContext(ed,config,EGL_NO_CONTEXT,ctx_attrs);
    if(es==EGL_NO_SURFACE||ctx==EGL_NO_CONTEXT||
       !eglMakeCurrent(ed,es,es,ctx))goto done;
    if(strcmp((const char *)glGetString(GL_VENDOR),"Grate"))goto done;
    puts("STREAM_GPU_READY Grate_Tegra=1 XR24=1 four_internal_passes=1");
    const char *frag=
      "precision mediump float;uniform mat4 m;uniform mat4 yuvm;"
      "uniform sampler2D sy;uniform sampler2D su;uniform sampler2D sv;"
      "varying vec2 uv;void main(){"
      "vec4 yuv1=vec4(texture2D(sy,uv).x,texture2D(su,uv).x,"
      "texture2D(sv,uv).x,1.0);"
      "vec4 q=vec4(dot(yuv1,yuvm[0]),dot(yuv1,yuvm[1]),"
      "dot(yuv1,yuvm[2]),1.0);gl_FragColor=m*q;}";
    program=make_program(frag);
    if(!program)goto done;
    const GLfloat coeff[3][4]={
        {1.164f,0.f,1.793f,-0.973051f},
        {1.164f,-0.213f,-0.533f,0.301427f},
        {1.164f,2.112f,0.f,-1.133176f}
    };
    GLfloat ym[16]={0};
    for(int i=0;i<3;i++)for(int j=0;j<4;j++)ym[i*4+j]=coeff[i][j];
    ym[15]=1.f;
    const GLfloat eye[16]={
        1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1};
    glUseProgram(program);
    glUniformMatrix4fv(glGetUniformLocation(program,"m"),1,GL_FALSE,eye);
    glUniformMatrix4fv(glGetUniformLocation(program,"yuvm"),1,GL_FALSE,ym);
    for(int i=0;i<3;i++)glUniform1i(glGetUniformLocation(program,
                             i==0?"sy":i==1?"su":"sv"),i);
    if(!check("stream-program-uniform"))goto done;
    stream=sl101_stream_open();
    if(!stream)goto done;
    printf("STREAM_MODE requested=%u audio_requested=%d audio_sink=pulsesink "
           "video_sync=1 audio_sync=1 cpu_yuv_map=0\n",target,audio);
    if(gst_element_set_state(pipeline,GST_STATE_PLAYING)==GST_STATE_CHANGE_FAILURE){
        nura_video_error(handle,"GStreamer could not start VDE pipeline");
        goto done;
    }
    nura_video_publish(handle,1,0,0,0);
    int paused=0;
    for(unsigned frame=1;frame<=target;){
        NuraVideoControls controls=nura_video_controls(handle);
        if(controls.stop){interrupted=1;break;}
        if(controls.volume_dirty){
            g_object_set(amplifier,"volume",controls.volume,
                         "mute",(gboolean)controls.muted,NULL);
        }
        if(controls.seek_ms>=0){
            gint64 ns=controls.seek_ms*GST_MSECOND;
            if(!gst_element_seek_simple(pipeline,GST_FORMAT_TIME,
                 GST_SEEK_FLAG_FLUSH|GST_SEEK_FLAG_KEY_UNIT,ns)){
                nura_video_error(handle,"VDE pipeline rejected seek");failed=1;
                break;
            }
            lastpts=GST_CLOCK_TIME_NONE;idle_ticks=0;
            held_position_ms=(uint64_t)controls.seek_ms;
            nura_video_publish(handle,paused?2:1,
                               (uint64_t)controls.seek_ms,duration_ms,count);
        }
        if(controls.pause){
            if(!paused){
                if(gst_element_set_state(pipeline,GST_STATE_PAUSED)==
                                      GST_STATE_CHANGE_FAILURE){
                    nura_video_error(handle,"VDE pipeline rejected pause");
                    failed=1;break;
                }
                paused=1;
            }
            nura_video_publish(handle,2,
                     GST_CLOCK_TIME_IS_VALID(lastpts)?
                     lastpts/GST_MSECOND:held_position_ms,duration_ms,count);
            g_usleep(25000);
            continue;
        }
        if(paused){
            if(gst_element_set_state(pipeline,GST_STATE_PLAYING)==
                                      GST_STATE_CHANGE_FAILURE){
                nura_video_error(handle,"VDE pipeline rejected resume");
                failed=1;break;
            }
            paused=0;
        }
        GstSample *sample=gst_app_sink_try_pull_sample(
                            GST_APP_SINK(sink),100*GST_MSECOND);
        if(!sample){
            if(gst_app_sink_is_eos(GST_APP_SINK(sink))){
                video_eos=1;
                printf("STREAM_VIDEO_EOS after_frames=%u\n",count);
                break;
            }
            if(check_bus(bus) || ++idle_ticks>=100){
                nura_video_error(handle,"VDE pipeline stalled or failed");
                failed=1;break;
            }
            continue;
        }
        idle_ticks=0;
        GstBuffer *buf=gst_sample_get_buffer(sample);
        GstClockTime pts=GST_BUFFER_PTS(buf);
        if(!GST_CLOCK_TIME_IS_VALID(pts)||
           (GST_CLOCK_TIME_IS_VALID(lastpts)&&pts<=lastpts))pts_bad++;
        if(GST_CLOCK_TIME_IS_VALID(lastpts)&&
           GST_CLOCK_TIME_IS_VALID(pts)&&
           pts-lastpts>GST_SECOND/16)pts_gap++;
        if(frame==1)firstpts=pts;
        lastpts=pts;
        int strides[3]={0},fds[3]={-1,-1,-1};
        if(!validate(sample,strides,fds,frame)){
            puts("STREAM_REJECT invalid-vde-dma-buffer");
            gst_sample_unref(sample);break;
        }
        const gint64 gpu_start=g_get_monotonic_time();
        GLuint tex[3]={0};
        EGLImageKHR images[3]={
            EGL_NO_IMAGE_KHR,EGL_NO_IMAGE_KHR,EGL_NO_IMAGE_KHR
        };
        if(!import_frame(ed,create,image_target,fds,strides,tex,images)){
            puts("STREAM_IMPORT_FAIL");
            forget_frame(ed,destroy,tex,images);
            gst_sample_unref(sample);break;
        }
        glDisable(GL_BLEND);
        glDisable(GL_SCISSOR_TEST);
        glDisable(GL_DEPTH_TEST);
        glClearColor(0,0,0,1);
        glClear(GL_COLOR_BUFFER_BIT);
        draw_nowait(program);
        glFinish(); /* finishes all four internal Grate draws before
                     * GstSample/EGLImage is permitted to be released */
        if(!check("stream-gpu-render")){
            forget_frame(ed,destroy,tex,images);
            gst_sample_unref(sample);break;
        }
        const gint64 gpu_end=g_get_monotonic_time();
        gpu_total_ms+=(gpu_end-gpu_start)/1000.0;
        if(g_getenv("NURAL_VIDEO_PIXEL_DIAGNOSTIC") &&
           (frame<=2 || frame==25 || frame==100 ||
            frame%24==0 || frame==target)){
            unsigned char center[3]={0};
            uint64_t hash=sample_hash(center);
            if(check("stream-pixel-hash")){
                if(checks && hash!=previous_hash)changed++;
                previous_hash=hash;checks++;
                printf("STREAM_PIXEL_SAMPLE frame=%u hash=%016llx distinct=%u "
                       "center_rgb=%u,%u,%u\n",
                       frame,(unsigned long long)hash,changed,
                       center[0],center[1],center[2]);
                if(!strcmp(nura_video_path(handle),
                       "/home/user/tds3e1-main.mp4") &&
                       (frame==1||frame==25||frame==100)){
                    const int expected[3]={
                        frame==1?0:frame==25?64:161,
                        frame==1?1:frame==25?72:180,
                        frame==1?0:frame==25?68:194
                    };
                    int ok=1;
                    for(int i=0;i<3;i++)
                        if(abs((int)center[i]-expected[i])>6)ok=0;
                    color_oracles++;
                    color_failures+=!ok;
                    printf("STREAM_COLOR_ORACLE frame=%u expected=%d,%d,%d "
                           "observed=%u,%u,%u tolerance=6 pass=%d\n",
                           frame,expected[0],expected[1],expected[2],
                           center[0],center[1],center[2],ok);
                }
            }
        }
        forget_frame(ed,destroy,tex,images);
        gst_sample_unref(sample);
        if(!eglSwapBuffers(ed,es)){
            puts("STREAM_EGL_SWAP_FAIL");break;
        }
        struct gbm_bo *bo=gbm_surface_lock_front_buffer(surface);
        if(!bo){puts("STREAM_GBM_LOCK_FAIL");break;}
        const int outfd=gbm_bo_get_fd(bo);
        if(outfd<0 || !sl101_stream_submit(stream,surface,bo,outfd,frame)){
            if(outfd>=0)close(outfd);
            puts("STREAM_WAYLAND_SUBMIT_FAIL");break;
        }
        close(outfd);
        const gint64 now=g_get_monotonic_time();
        if(frame==1)first_us=now;
        last_us=now;
        GstClock *clock=gst_element_get_clock(pipeline);
        if(clock && GST_CLOCK_TIME_IS_VALID(pts)){
            GstClockTime running=gst_clock_get_time(clock)-
                                  gst_element_get_base_time(pipeline);
            double lag_ms=((gint64)running-(gint64)pts)/1000000.0;
            if(lag_ms>post_pts_lag_max_ms)post_pts_lag_max_ms=lag_ms;
            post_pts_lag_sum_ms+=lag_ms;
        }
        if(clock)gst_object_unref(clock);
        count++;
        frame++;
        held_position_ms=pts/GST_MSECOND;
        gint64 total_duration=0;
        if(gst_element_query_duration(pipeline,GST_FORMAT_TIME,&total_duration)
           && total_duration>0)duration_ms=(uint64_t)total_duration/GST_MSECOND;
        nura_video_publish(handle,1,pts/GST_MSECOND,duration_ms,count);
        if(frame<=2||frame%24==0||frame==target)
            printf("STREAM_FRAME frame=%u pts_ns=%" G_GUINT64_FORMAT
                   " gpu_ms=%.3f clock_audio=%d wayland_callback=%u released=%u\n",
                   frame,pts,(gpu_end-gpu_start)/1000.0,audio,
                   sl101_stream_callbacks(stream),sl101_stream_releases(stream));
        if(check_bus(bus))break;
    }
    const double sec=(first_us&&last_us>first_us)?
                  (last_us-first_us)/1000000.0:0.0;
    const double presented_fps=sec>0?(count-1)/sec:0.0;
    const double media_sec=GST_CLOCK_TIME_IS_VALID(firstpts)&&
                  GST_CLOCK_TIME_IS_VALID(lastpts)&&lastpts>=firstpts?
                  (lastpts-firstpts)/(double)GST_SECOND:0.0;
    if(until_eos&&video_eos){
        GstMessage *m=gst_bus_timed_pop_filtered(bus,4*GST_SECOND,
                         GST_MESSAGE_EOS|GST_MESSAGE_ERROR);
        if(m){
            if(GST_MESSAGE_TYPE(m)==GST_MESSAGE_EOS) {
                pipeline_eos=1;
                puts("STREAM_PIPELINE_EOS video_and_audio_drained=1");
            }else{
                GError *e=NULL;gchar *details=NULL;
                gst_message_parse_error(m,&e,&details);
                printf("STREAM_GST_ERROR after_video_eos=%s\n",
                       e?e->message:"unknown");
                g_clear_error(&e);g_free(details);
            }
            gst_message_unref(m);
        }else puts("STREAM_PIPELINE_EOS_TIMEOUT");
    }
    int bus_bad=check_bus(bus);
    const unsigned released=stream?sl101_stream_releases(stream):0;
    const unsigned callbacks=stream?sl101_stream_callbacks(stream):0;
    const int stream_clean=sl101_stream_close(stream);
    stream=NULL;
    fflush(stdout);
    const int transport_ok=(until_eos?
               video_eos&&pipeline_eos:count==target)&&
               !pts_bad&&!pts_gap&&!color_failures&&
               !errors&&stream_clean&&!bus_bad&&!failed;
    printf("STREAM_RESULT requested=%u presented=%u distinct_sampled=%u "
           "pixel_checks=%u color_oracles=%u color_failures=%u "
           "pts_bad=%u pts_gaps=%u audio_branch=%d "
           "until_eos=%d video_eos=%d pipeline_eos=%d "
           "wayland_callbacks=%u releases_before_close=%u "
           "gpu_ms_total=%.3f elapsed_present_s=%.3f media_s=%.3f "
           "fps_presented=%.3f av_clock_lag_mean_ms=%.3f av_clock_lag_max_ms=%.3f "
           "stream_cleanup_ok=%d bus_errors=%d cpu_yuv_map=0 "
           "sustained_video_qualified=%d\n",
           until_eos?0:target,count,changed,checks,color_oracles,
           color_failures,pts_bad,pts_gap,audio,
           until_eos,video_eos,pipeline_eos,
           callbacks,released,gpu_total_ms,sec,media_sec,presented_fps,
           count?post_pts_lag_sum_ms/count:0,post_pts_lag_max_ms,
           stream_clean,bus_bad,transport_ok);
    fflush(stdout);
    ret=transport_ok||interrupted?0:7;
    if(transport_ok)
        nura_video_publish(handle,3,
                GST_CLOCK_TIME_IS_VALID(lastpts)?lastpts/GST_MSECOND:0,
                duration_ms,count);
done:
    if(stream)sl101_stream_close(stream);
    if(pipeline)gst_element_set_state(pipeline,GST_STATE_NULL);
    if(program && ed!=EGL_NO_DISPLAY)glDeleteProgram(program);
    if(ed!=EGL_NO_DISPLAY && ctx!=EGL_NO_CONTEXT)
        eglMakeCurrent(ed,EGL_NO_SURFACE,EGL_NO_SURFACE,EGL_NO_CONTEXT);
    if(ctx!=EGL_NO_CONTEXT)eglDestroyContext(ed,ctx);
    if(es!=EGL_NO_SURFACE)eglDestroySurface(ed,es);
    if(surface)gbm_surface_destroy(surface);
    if(ed!=EGL_NO_DISPLAY)eglTerminate(ed);
    if(gbm)gbm_device_destroy(gbm);
    if(devfd>=0)close(devfd);
    if(bus)gst_object_unref(bus);
    if(sink)gst_object_unref(sink);
    if(amplifier)gst_object_unref(amplifier);
    if(pipeline)gst_object_unref(pipeline);
    if(ret && !interrupted && !video_eos)
        nura_video_error(handle,"VDE/GPU/Wayland video setup or rendering failed");
    return ret;
}
