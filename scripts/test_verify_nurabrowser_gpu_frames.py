import unittest

from verify_nurabrowser_gpu_frames import verify


def heartbeat(wall, time, frames, presented, media_time, total, width=640):
    return (f'GET /heartbeat?wall={wall}&time={time}&frames={frames}'
            f'&presented={presented}&mediaTime={media_time}&total={total}'
            f'&width={width}&height=360&ready=4 HTTP/1.1')


GST_GL = ('GstGLContext EGL activated\n'
          'caps video/x-raw(memory:GLMemory),format=RGBA\n'
          'GstGLMemory render buffer\n')


class GpuFramesTest(unittest.TestCase):
    def setUp(self):
        self.samples = '\n'.join((heartbeat(0, 1, 1, 1, 1, 1),
                                  heartbeat(15, 16, 330, 330, 16, 330)))

    def test_gpu_frames_advance(self):
        self.assertTrue(verify(self.samples, GST_GL)[0])

    def test_no_frames(self):
        self.assertFalse(verify('\n'.join((heartbeat(0, 1, 1, 1, 1, 1),
                                           heartbeat(15, 1, 1, 1, 1, 1))), GST_GL)[0])

    def test_software_sink_is_not_gpu(self):
        self.assertFalse(verify(self.samples, 'video/x-raw,format=I420\n')[0])
        self.assertFalse(verify(self.samples, GST_GL + 'GALLIUM_DRIVER=softpipe')[0])

    def test_invalid_caps(self):
        self.assertFalse(verify(self.samples, GST_GL.replace('GLMemory', 'SystemMemory'))[0])

    def test_context_loss_or_missing_context(self):
        self.assertFalse(verify(self.samples, GST_GL.replace('GstGLContext EGL activated', 'EGL_FAILED'))[0])
        self.assertFalse(verify(self.samples, GST_GL + 'GstGLContext context lost')[0])

    def test_browser_error(self):
        self.assertFalse(verify(self.samples + '&error=3', GST_GL)[0])

    def test_no_heartbeat(self):
        self.assertFalse(verify('', GST_GL)[0])

    def test_still_image(self):
        self.assertFalse(verify('\n'.join((heartbeat(0, 1, 1, 1, 1, 1),
                                           heartbeat(15, 1, 2, 2, 1, 2))), GST_GL)[0])


if __name__ == '__main__':
    unittest.main()
