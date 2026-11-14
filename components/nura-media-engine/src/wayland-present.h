/* Private, bounded Wayland presentation oracle for one exported RGB DMABuf. */
#ifndef SL101_WAYLAND_PRESENT_H
#define SL101_WAYLAND_PRESENT_H
#include <stdint.h>
int sl101_wayland_present_dmabuf(int fd,uint32_t fourcc,uint64_t modifier,
                                int stride,int width,int height);
#endif
