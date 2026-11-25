// The overlay pass's output is premultiplied (libplacebo's overlay blend onto transparent black);
// Skathi composites a surface as straight alpha (sk-tree surface.rs: "straight alpha"; gpui's
// sprite pipeline blends SourceAlpha / OneMinusSourceAlpha). This pass divides rgb by alpha in
// place, on the GPU, between mpv's release and Skathi's copy, on the slot's own timeline:
//
//   mpv signals ready (v + 1) -> this pass waits v + 1, writes, signals v + 2 -> Skathi waits v + 2
//
// Nothing waits on the CPU. Built with ARC.

#import <Metal/Metal.h>
#import <IOSurface/IOSurface.h>

static NSString *const kSource =
    @"#include <metal_stdlib>\n"
     "using namespace metal;\n"
     "static half3 enc(half3 l) {\n"
     "  float3 c = clamp(float3(l), 0.0, 1.0);\n"
     "  return half3(select(1.055 * pow(c, 1.0 / 2.4) - 0.055, 12.92 * c, c <= 0.0031308));\n"
     "}\n"
     "static half4 fix(half4 c, uint mode) {\n"
     "  if (c.a <= 0.0h) return half4(0.0h);\n"
     "  if (c.a < 1.0h) c.rgb = c.rgb / c.a;\n"
     "  if (mode & 1u) c.rgb = enc(c.rgb);\n"
     "  return c;\n"
     "}\n"
     "kernel void unpremul(texture2d<half, access::read_write> t [[texture(0)]],\n"
     "                     constant uint2 &p [[buffer(0)]],\n"
     "                     uint2 g [[thread_position_in_grid]]) {\n"
     "  if (g.x >= t.get_width() || g.y >= t.get_height()) return;\n"
     "  half4 c = t.read(g);\n"
     "  if (c.a < 1.0h || (p.y & 1u)) t.write(fix(c, p.y), g);\n"
     "}\n"
     "kernel void band(texture2d<half, access::read> s [[texture(0)]],\n"
     "                 texture2d<half, access::write> d [[texture(1)]],\n"
     "                 constant uint2 &p [[buffer(0)]],\n"
     "                 uint2 g [[thread_position_in_grid]]) {\n"
     "  if (g.x >= d.get_width() || g.y >= d.get_height()) return;\n"
     "  d.write(fix(s.read(uint2(g.x, g.y + p.x)), p.y), g);\n"
     "}\n"
     "kernel void scan(texture2d<half, access::read> s [[texture(0)]],\n"
     "                 device atomic_uint *box [[buffer(0)]],\n"
     "                 uint2 g [[thread_position_in_grid]]) {\n"
     "  if (g.x >= s.get_width() || g.y >= s.get_height()) return;\n"
     "  if (s.read(g).a > 0.0h) {\n"
     "    atomic_fetch_min_explicit(&box[0], g.y, memory_order_relaxed);\n"
     "    atomic_fetch_max_explicit(&box[1], g.y, memory_order_relaxed);\n"
     "  }\n"
     "}\n";

@interface FpuiUnpremul : NSObject
@property(strong) id<MTLDevice> device;
@property(strong) id<MTLCommandQueue> queue;
@property(strong) id<MTLComputePipelineState> pipeline;
@property(strong) id<MTLComputePipelineState> band;
@property(strong) id<MTLComputePipelineState> scan;
// The rows the last completed scan found drawn, top in the high half, bottom in the
// low half; UINT64_MAX when none was found or none has completed.
@property(atomic) uint64_t lastBox;
@property(atomic) uint64_t scans;
@end
@implementation FpuiUnpremul
@end

// Returns a retained context, or NULL (the message goes to stderr).
// `metallib`/`len`, when given, is kSource precompiled at build time (build.rs).
void *fpui_unpremul_new(const void *metallib, size_t len) {
    @autoreleasepool {
        FpuiUnpremul *u = [FpuiUnpremul new];
        u.device = MTLCreateSystemDefaultDevice();
        if (!u.device) return NULL;
        NSError *err = nil;
        id<MTLLibrary> lib = nil;
        if (metallib && len) {
            dispatch_data_t data = dispatch_data_create(metallib, len, NULL, DISPATCH_DATA_DESTRUCTOR_DEFAULT);
            lib = [u.device newLibraryWithData:data error:&err];
            if (!lib) fprintf(stderr, "[fpui unpremul] precompiled library refused: %s\n", err.localizedDescription.UTF8String);
        }
        if (!lib) lib = [u.device newLibraryWithSource:kSource options:nil error:&err];
        if (!lib) {
            fprintf(stderr, "[fpui unpremul] %s\n", err.localizedDescription.UTF8String);
            return NULL;
        }
        u.pipeline = [u.device newComputePipelineStateWithFunction:[lib newFunctionWithName:@"unpremul"] error:&err];
        if (!u.pipeline) {
            fprintf(stderr, "[fpui unpremul] %s\n", err.localizedDescription.UTF8String);
            return NULL;
        }
        u.band = [u.device newComputePipelineStateWithFunction:[lib newFunctionWithName:@"band"] error:&err];
        if (!u.band) {
            fprintf(stderr, "[fpui unpremul] %s\n", err.localizedDescription.UTF8String);
            return NULL;
        }
        u.scan = [u.device newComputePipelineStateWithFunction:[lib newFunctionWithName:@"scan"] error:&err];
        if (!u.scan) {
            fprintf(stderr, "[fpui unpremul] %s\n", err.localizedDescription.UTF8String);
            return NULL;
        }
        u.lastBox = UINT64_MAX;
        u.queue = [u.device newCommandQueue];
        u.queue.label = @"fpui overlay unpremultiply";
        return (__bridge_retained void *)u;
    }
}

void fpui_unpremul_free(void *ctx) {
    if (ctx) (void)(__bridge_transfer FpuiUnpremul *)ctx;
}

// A read-write view of one ring slot's IOSurface (RGBA16F). Retained; free with fpui_release.
// `eight`: a 'BGRA' surface, else 'RGhA'.
void *fpui_unpremul_texture(void *ctx, IOSurfaceRef surface, int w, int h, int eight) {
    FpuiUnpremul *u = (__bridge FpuiUnpremul *)ctx;
    MTLTextureDescriptor *d = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:(eight ? MTLPixelFormatBGRA8Unorm : MTLPixelFormatRGBA16Float)
                                                                                 width:w height:h mipmapped:NO];
    d.usage = MTLTextureUsageShaderRead | MTLTextureUsageShaderWrite;
    d.storageMode = MTLStorageModeShared;
    id<MTLTexture> t = [u.device newTextureWithDescriptor:d iosurface:surface plane:0];
    return t ? (__bridge_retained void *)t : NULL;
}

void fpui_release(void *obj) {
    if (obj) CFRelease(obj);
}

// GPU: wait `event` >= wait, un-premultiply (mode bit 0: also sRGB-encode, for an 8-bit target
// that holds linear values) `texture` in place, or, with `dest`, rows offset_y.. of `texture`
// into `dest` (the band); signal `event` = signal. Returns 0 on commit.
// With `scan`, the same command buffer also finds the first and last row of `texture`
// with any alpha (read back with fpui_unpremul_box once it completes).
int fpui_unpremul_run(void *ctx, void *texture, void *dest, void *event, uint64_t wait, uint64_t signal,
                      uint32_t offset_y, uint32_t mode, int scan) {
    @autoreleasepool {
        FpuiUnpremul *u = (__bridge FpuiUnpremul *)ctx;
        id<MTLTexture> t = (__bridge id<MTLTexture>)texture;
        id<MTLTexture> d = dest ? (__bridge id<MTLTexture>)dest : nil;
        id<MTLSharedEvent> e = (__bridge id<MTLSharedEvent>)event;
        id<MTLCommandBuffer> cb = [u.queue commandBufferWithUnretainedReferences];
        if (!cb) return -1;
        [cb encodeWaitForEvent:e value:wait];
        id<MTLComputeCommandEncoder> enc = [cb computeCommandEncoder];
        id<MTLComputePipelineState> ps = d ? u.band : u.pipeline;
        [enc setComputePipelineState:ps];
        [enc setTexture:t atIndex:0];
        if (d) [enc setTexture:d atIndex:1];
        uint32_t p[2] = {offset_y, mode};
        [enc setBytes:p length:sizeof p atIndex:0];
        id<MTLTexture> grid = d ? d : t;
        NSUInteger tw = ps.threadExecutionWidth;
        NSUInteger th = ps.maxTotalThreadsPerThreadgroup / tw;
        [enc dispatchThreads:MTLSizeMake(grid.width, grid.height, 1) threadsPerThreadgroup:MTLSizeMake(tw, th, 1)];
        id<MTLBuffer> box = nil;
        if (scan) {
            box = [u.device newBufferWithLength:8 options:MTLResourceStorageModeShared];
            uint32_t *b = (uint32_t *)box.contents;
            b[0] = UINT32_MAX;
            b[1] = 0;
            [enc setComputePipelineState:u.scan];
            [enc setTexture:t atIndex:0];
            [enc setBuffer:box offset:0 atIndex:0];
            NSUInteger sw = u.scan.threadExecutionWidth, sh = u.scan.maxTotalThreadsPerThreadgroup / sw;
            [enc dispatchThreads:MTLSizeMake(t.width, t.height, 1) threadsPerThreadgroup:MTLSizeMake(sw, sh, 1)];
        }
        [enc endEncoding];
        [cb encodeSignalEvent:e value:signal];
        if (box) {
            __weak FpuiUnpremul *weak = u;
            [cb addCompletedHandler:^(id<MTLCommandBuffer> done) {
                (void)done;
                FpuiUnpremul *s = weak;
                if (!s) return;
                const uint32_t *b = (const uint32_t *)box.contents;
                s.lastBox = b[0] == UINT32_MAX ? UINT64_MAX : (((uint64_t)b[0] << 32) | b[1]);
                s.scans = s.scans + 1;
            }];
        }
        [cb commit];
        return 0;
    }
}

// The last completed scan (see fpui_unpremul_run) and how many have completed.
uint64_t fpui_unpremul_box(void *ctx, uint64_t *scans) {
    FpuiUnpremul *u = (__bridge FpuiUnpremul *)ctx;
    if (scans) *scans = u.scans;
    return u.lastBox;
}
