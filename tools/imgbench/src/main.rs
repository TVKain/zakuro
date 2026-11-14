//! imgbench: runs Zakuro's screen shader off-screen and scores pictures.
//!
//!   imgbench present <in> <W> <H> <outprefix> <modes> [--spv f] [--reps n] [--srgb]
//!   imgbench ref <in> <W> <H> <out>           Lanczos3 in linear light
//!   imgbench cmp <test> <reference>           PSNR, SSIM, edge energy ratio
//!   imgbench crop <in> <x> <y> <w> <h> <zoom> <out>

use ash::vk;
use image::{Rgb, RgbImage};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("present") => present(&args[1..]),
        Some("ref") => {
            let img = load(&args[1]);
            let (w, h) = (args[2].parse().unwrap(), args[3].parse().unwrap());
            BOX.store(args.get(5).is_some_and(|a| a == "box"), std::sync::atomic::Ordering::Relaxed);
            reference(&img, w, h).save(&args[4]).unwrap();
        }
        Some("cmp") => {
            let (a, b) = (load(&args[1]), load(&args[2]));
            let (psnr, ssim, edges) = compare(&a, &b);
            println!("psnr {psnr:.3} ssim {ssim:.5} edges {edges:.4}");
        }
        Some("crop") => {
            let img = load(&args[1]);
            let p: Vec<u32> = args[2..7].iter().map(|a| a.parse().unwrap()).collect();
            let out = RgbImage::from_fn(p[2] * p[4], p[3] * p[4], |x, y| *img.get_pixel(p[0] + x / p[4], p[1] + y / p[4]));
            out.save(&args[7]).unwrap();
        }
        _ => eprintln!("see the top of main.rs"),
    }
}

fn load(path: &str) -> RgbImage {
    image::open(path).unwrap_or_else(|e| panic!("{path}: {e}")).to_rgb8()
}

fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

static BOX: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn lanczos(x: f32, a: f32) -> f32 {
    if x == 0.0 {
        return 1.0;
    }
    if x.abs() >= a {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    a * px.sin() * (px / a).sin() / (px * px)
}

/// one axis of a separable resample, kernel widened when shrinking.
fn resample_axis(src: &[f32], sw: usize, sh: usize, dw: usize, horizontal: bool) -> Vec<f32> {
    let (n_in, lines) = if horizontal { (sw, sh) } else { (sh, sw) };
    let ratio = n_in as f32 / dw as f32;
    let scale = ratio.max(1.0);
    let a = 3.0;
    let support = a * scale;
    let mut out = vec![0.0; if horizontal { dw * sh * 3 } else { sw * dw * 3 }];
    let boxed = BOX.load(std::sync::atomic::Ordering::Relaxed);
    for o in 0..dw {
        if boxed {
            let (a0, a1) = (o as f32 * ratio, (o + 1) as f32 * ratio);
            let mut weights = Vec::new();
            for i in (a0.floor() as usize)..(a1.ceil() as usize).min(n_in) {
                let w = (a1.min(i as f32 + 1.0) - a0.max(i as f32)).max(0.0);
                if w > 0.0 { weights.push((i, w)); }
            }
            let sum: f32 = weights.iter().map(|w| w.1).sum();
            for line in 0..lines {
                let mut acc = [0.0f32; 3];
                for &(i, w) in &weights {
                    let idx = if horizontal { (line * sw + i) * 3 } else { (i * sw + line) * 3 };
                    for c in 0..3 { acc[c] += src[idx + c] * w; }
                }
                let idx = if horizontal { (line * dw + o) * 3 } else { (o * sw + line) * 3 };
                for c in 0..3 { out[idx + c] = acc[c] / sum; }
            }
            continue;
        }
        let centre = (o as f32 + 0.5) * ratio - 0.5;
        let lo = (centre - support).floor() as i64;
        let hi = (centre + support).ceil() as i64;
        let mut weights = Vec::new();
        let mut sum = 0.0;
        for i in lo..=hi {
            let w = lanczos((i as f32 - centre) / scale, a);
            if w != 0.0 {
                weights.push((i.clamp(0, n_in as i64 - 1) as usize, w));
                sum += w;
            }
        }
        for line in 0..lines {
            let mut acc = [0.0f32; 3];
            for &(i, w) in &weights {
                let idx = if horizontal { (line * sw + i) * 3 } else { (i * sw + line) * 3 };
                for c in 0..3 {
                    acc[c] += src[idx + c] * w;
                }
            }
            let idx = if horizontal { (line * dw + o) * 3 } else { (o * sw + line) * 3 };
            for c in 0..3 {
                out[idx + c] = acc[c] / sum;
            }
        }
    }
    out
}

fn reference(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    let (sw, sh) = (img.width() as usize, img.height() as usize);
    let lin: Vec<f32> = img.as_raw().iter().map(|&c| to_linear(c)).collect();
    let across = resample_axis(&lin, sw, sh, w as usize, true);
    let down = resample_axis(&across, w as usize, sh, h as usize, false);
    RgbImage::from_fn(w, h, |x, y| {
        let i = ((y * w + x) * 3) as usize;
        Rgb([to_srgb(down[i]), to_srgb(down[i + 1]), to_srgb(down[i + 2])])
    })
}

fn luma(img: &RgbImage) -> Vec<f32> {
    img.pixels().map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).collect()
}

fn compare(a: &RgbImage, b: &RgbImage) -> (f64, f64, f64) {
    assert_eq!(a.dimensions(), b.dimensions(), "sizes differ");
    let (w, h) = (a.width() as usize, a.height() as usize);
    let mse: f64 = a.as_raw().iter().zip(b.as_raw()).map(|(&x, &y)| (x as f64 - y as f64).powi(2)).sum::<f64>() / (w * h * 3) as f64;
    let psnr = 10.0 * (255.0f64 * 255.0 / mse.max(1e-9)).log10();
    let (la, lb) = (luma(a), luma(b));
    // SSIM with an 11x11 Gaussian window, sigma 1.5
    let g: Vec<f32> = (-5..=5).map(|i: i32| (-(i * i) as f32 / (2.0 * 1.5 * 1.5)).exp()).collect();
    let gs: f32 = g.iter().sum();
    let g: Vec<f32> = g.iter().map(|v| v / gs).collect();
    let blur = |src: &[f32]| -> Vec<f32> {
        let mut t = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for (k, wt) in g.iter().enumerate() {
                    let xx = (x as i64 + k as i64 - 5).clamp(0, w as i64 - 1) as usize;
                    s += src[y * w + xx] * wt;
                }
                t[y * w + x] = s;
            }
        }
        let mut o = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for (k, wt) in g.iter().enumerate() {
                    let yy = (y as i64 + k as i64 - 5).clamp(0, h as i64 - 1) as usize;
                    s += t[yy * w + x] * wt;
                }
                o[y * w + x] = s;
            }
        }
        o
    };
    let ma = blur(&la);
    let mb = blur(&lb);
    let aa: Vec<f32> = la.iter().map(|v| v * v).collect();
    let bb: Vec<f32> = lb.iter().map(|v| v * v).collect();
    let ab: Vec<f32> = la.iter().zip(&lb).map(|(x, y)| x * y).collect();
    let (saa, sbb, sab) = (blur(&aa), blur(&bb), blur(&ab));
    let (c1, c2) = ((0.01f32 * 255.0).powi(2), (0.03f32 * 255.0).powi(2));
    let mut ssim = 0.0f64;
    for i in 0..w * h {
        let (mx, my) = (ma[i], mb[i]);
        let vx = saa[i] - mx * mx;
        let vy = sbb[i] - my * my;
        let cxy = sab[i] - mx * my;
        ssim += (((2.0 * mx * my + c1) * (2.0 * cxy + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2))) as f64;
    }
    let edges = |l: &[f32]| -> f64 {
        let mut e = 0.0f64;
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                e += ((l[y * w + x + 1] - l[y * w + x]).abs() + (l[(y + 1) * w + x] - l[y * w + x]).abs()) as f64;
            }
        }
        e
    };
    (psnr, ssim / (w * h) as f64, edges(&la) / edges(&lb))
}

// ---------------------------------------------------------------- Vulkan

struct Gpu {
    _entry: ash::Entry,
    instance: ash::Instance,
    device: ash::Device,
    queue: vk::Queue,
    family: u32,
    memory: vk::PhysicalDeviceMemoryProperties,
    period: f32,
}

impl Gpu {
    fn new() -> Gpu {
        unsafe {
            let entry = ash::Entry::load().expect("vulkan loader");
            let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3);
            let instance = entry.create_instance(&vk::InstanceCreateInfo::default().application_info(&app), None).unwrap();
            let physical = instance.enumerate_physical_devices().unwrap()[0];
            let props = instance.get_physical_device_properties(physical);
            eprintln!("gpu: {:?}", props.device_name_as_c_str().unwrap());
            let family = instance
                .get_physical_device_queue_family_properties(physical)
                .iter()
                .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .unwrap() as u32;
            let prio = [1.0];
            let queues = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&prio)];
            let device = instance.create_device(physical, &vk::DeviceCreateInfo::default().queue_create_infos(&queues), None).unwrap();
            let queue = device.get_device_queue(family, 0);
            let memory = instance.get_physical_device_memory_properties(physical);
            Gpu { _entry: entry, instance, device, queue, family, memory, period: props.limits.timestamp_period }
        }
    }

    fn mem_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> u32 {
        (0..self.memory.memory_type_count)
            .find(|&i| bits & (1 << i) != 0 && self.memory.memory_types[i as usize].property_flags.contains(want))
            .unwrap()
    }

    fn buffer(&self, size: u64, usage: vk::BufferUsageFlags) -> (vk::Buffer, vk::DeviceMemory, *mut u8) {
        unsafe {
            let buf = self.device.create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage), None).unwrap();
            let req = self.device.get_buffer_memory_requirements(buf);
            let ty = self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT);
            let mem = self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None).unwrap();
            self.device.bind_buffer_memory(buf, mem, 0).unwrap();
            let ptr = self.device.map_memory(mem, 0, size, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
            (buf, mem, ptr)
        }
    }

    fn image(&self, w: u32, h: u32, usage: vk::ImageUsageFlags, mutable: bool) -> vk::Image {
        unsafe {
            let flags = if mutable { vk::ImageCreateFlags::MUTABLE_FORMAT } else { vk::ImageCreateFlags::empty() };
            let img = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .flags(flags)
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_UNORM)
                        .extent(vk::Extent3D { width: w, height: h, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(usage),
                    None,
                )
                .unwrap();
            let req = self.device.get_image_memory_requirements(img);
            let ty = self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL);
            let mem = self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(ty), None).unwrap();
            self.device.bind_image_memory(img, mem, 0).unwrap();
            img
        }
    }

    fn view(&self, img: vk::Image, format: vk::Format) -> vk::ImageView {
        unsafe {
            self.device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(img)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 }),
                    None,
                )
                .unwrap()
        }
    }
}

fn barrier(d: &ash::Device, cb: vk::CommandBuffer, img: vk::Image, from: vk::ImageLayout, to: vk::ImageLayout) {
    let b = vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
        .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
        .old_layout(from)
        .new_layout(to)
        .image(img)
        .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 });
    unsafe { d.cmd_pipeline_barrier(cb, vk::PipelineStageFlags::ALL_COMMANDS, vk::PipelineStageFlags::ALL_COMMANDS, vk::DependencyFlags::empty(), &[], &[], &[b]) };
}

fn spirv(path: &str) -> Vec<u32> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    bytes.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn present(args: &[String]) {
    let input = load(&args[0]);
    let (ow, oh): (u32, u32) = (args[1].parse().unwrap(), args[2].parse().unwrap());
    let prefix = &args[3];
    let modes: Vec<i32> = args[4].split(',').map(|m| m.parse().unwrap()).collect();
    // the repo's own shaders unless --spv names another build
    let shaders = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/zakuro-gpu/shaders");
    let mut spv = format!("{shaders}/present.frag.spv");
    let mut vert = format!("{shaders}/present.vert.spv");
    let mut reps = 200u32;
    let mut srgb = false;
    let mut i = 5;
    while i < args.len() {
        match args[i].as_str() {
            "--spv" => { spv = args[i + 1].clone(); i += 1; }
            "--vert" => { vert = args[i + 1].clone(); i += 1; }
            "--reps" => { reps = args[i + 1].parse().unwrap(); i += 1; }
            "--srgb" => srgb = true,
            other => panic!("unknown {other}"),
        }
        i += 1;
    }
    let gpu = Gpu::new();
    let d = &gpu.device;
    let (iw, ih) = input.dimensions();
    unsafe {
        let pool = d.create_command_pool(&vk::CommandPoolCreateInfo::default().queue_family_index(gpu.family), None).unwrap();
        let cb = d.allocate_command_buffers(&vk::CommandBufferAllocateInfo::default().command_pool(pool).command_buffer_count(1)).unwrap()[0];
        // source
        let src = gpu.image(iw, ih, vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST, true);
        let src_view = gpu.view(src, if srgb { vk::Format::R8G8B8A8_SRGB } else { vk::Format::R8G8B8A8_UNORM });
        let (up, _, up_ptr) = gpu.buffer((iw * ih * 4) as u64, vk::BufferUsageFlags::TRANSFER_SRC);
        for (k, p) in input.pixels().enumerate() {
            std::ptr::copy_nonoverlapping([p[0], p[1], p[2], 255].as_ptr(), up_ptr.add(k * 4), 4);
        }
        // target
        let dst = gpu.image(ow, oh, vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC, false);
        let dst_view = gpu.view(dst, vk::Format::R8G8B8A8_UNORM);
        let (down, _, down_ptr) = gpu.buffer((ow * oh * 4) as u64, vk::BufferUsageFlags::TRANSFER_DST);
        let attach = [vk::AttachmentDescription::default()
            .format(vk::Format::R8G8B8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)];
        let refs = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
        let sub = [vk::SubpassDescription::default().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&refs)];
        let rp = d.create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&attach).subpasses(&sub), None).unwrap();
        let views = [dst_view];
        let fb = d.create_framebuffer(&vk::FramebufferCreateInfo::default().render_pass(rp).attachments(&views).width(ow).height(oh).layers(1), None).unwrap();
        let sampler = d
            .create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                None,
            )
            .unwrap();
        let bind = [vk::DescriptorSetLayoutBinding::default().binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).descriptor_count(1).stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let dsl = d.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bind), None).unwrap();
        let sizes = [vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 1 }];
        let dp = d.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(1).pool_sizes(&sizes), None).unwrap();
        let dsls = [dsl];
        let set = d.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(dp).set_layouts(&dsls)).unwrap()[0];
        let info = [vk::DescriptorImageInfo::default().sampler(sampler).image_view(src_view).image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        d.update_descriptor_sets(&[vk::WriteDescriptorSet::default().dst_set(set).dst_binding(0).descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER).image_info(&info)], &[]);
        let ranges = [
            vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::VERTEX).offset(0).size(4),
            vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::FRAGMENT).offset(16).size(40),
        ];
        let layout = d.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&dsls).push_constant_ranges(&ranges), None).unwrap();
        let vs = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv(&vert)), None).unwrap();
        let fs = d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv(&spv)), None).unwrap();
        let stages = [
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(vs).name(c"main"),
            vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(fs).name(c"main"),
        ];
        let vi = vk::PipelineVertexInputStateCreateInfo::default();
        let ia = vk::PipelineInputAssemblyStateCreateInfo::default().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let vp = [vk::Viewport { x: 0.0, y: 0.0, width: ow as f32, height: oh as f32, min_depth: 0.0, max_depth: 1.0 }];
        let sc = [vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: vk::Extent2D { width: ow, height: oh } }];
        let vps = vk::PipelineViewportStateCreateInfo::default().viewports(&vp).scissors(&sc);
        let rs = vk::PipelineRasterizationStateCreateInfo::default().polygon_mode(vk::PolygonMode::FILL).cull_mode(vk::CullModeFlags::NONE).line_width(1.0);
        let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let ba = [vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::RGBA)];
        let bs = vk::PipelineColorBlendStateCreateInfo::default().attachments(&ba);
        let pinfo = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vi)
            .input_assembly_state(&ia)
            .viewport_state(&vps)
            .rasterization_state(&rs)
            .multisample_state(&ms)
            .color_blend_state(&bs)
            .layout(layout)
            .render_pass(rp)
            .subpass(0);
        let pipeline = d.create_graphics_pipelines(vk::PipelineCache::null(), &[pinfo], None).unwrap()[0];
        let qp = d.create_query_pool(&vk::QueryPoolCreateInfo::default().query_type(vk::QueryType::TIMESTAMP).query_count(2), None).unwrap();
        let fence = d.create_fence(&vk::FenceCreateInfo::default(), None).unwrap();
        let mut first = true;
        for &mode in &modes {
            d.reset_command_buffer(cb, vk::CommandBufferResetFlags::empty()).unwrap();
            d.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default()).unwrap();
            if first {
                barrier(d, cb, src, vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
                d.cmd_copy_buffer_to_image(cb, up, src, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 })
                    .image_extent(vk::Extent3D { width: iw, height: ih, depth: 1 })]);
                barrier(d, cb, src, vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
                first = false;
            }
            d.cmd_reset_query_pool(cb, qp, 0, 2);
            let mut pc: Vec<u8> = Vec::new();
            let (hx, hy) = (0.5 / iw as f32, 0.5 / ih as f32);
            for f in [0.0f32, 0.0, 1.0, 1.0, hx, hy, 1.0 - hx, 1.0 - hy] {
                pc.extend(f.to_le_bytes());
            }
            pc.extend(mode.to_le_bytes());
            pc.extend(0i32.to_le_bytes());
            let clear = [vk::ClearValue { color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] } }];
            // one draw per render pass, timed over all of them
            for r in 0..reps.max(1) {
                d.cmd_begin_render_pass(cb, &vk::RenderPassBeginInfo::default().render_pass(rp).framebuffer(fb).render_area(sc[0]).clear_values(&clear), vk::SubpassContents::INLINE);
                if r == 0 {
                    d.cmd_write_timestamp(cb, vk::PipelineStageFlags::TOP_OF_PIPE, qp, 0);
                }
                d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, pipeline);
                d.cmd_bind_descriptor_sets(cb, vk::PipelineBindPoint::GRAPHICS, layout, 0, &[set], &[]);
                d.cmd_push_constants(cb, layout, vk::ShaderStageFlags::VERTEX, 0, &0i32.to_le_bytes());
                d.cmd_push_constants(cb, layout, vk::ShaderStageFlags::FRAGMENT, 16, &pc);
                d.cmd_draw(cb, 3, 1, 0, 0);
                d.cmd_end_render_pass(cb);
            }
            d.cmd_write_timestamp(cb, vk::PipelineStageFlags::BOTTOM_OF_PIPE, qp, 1);
            d.cmd_copy_image_to_buffer(cb, dst, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, down, &[vk::BufferImageCopy::default()
                .image_subresource(vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 })
                .image_extent(vk::Extent3D { width: ow, height: oh, depth: 1 })]);
            d.end_command_buffer(cb).unwrap();
            let cbs = [cb];
            d.queue_submit(gpu.queue, &[vk::SubmitInfo::default().command_buffers(&cbs)], fence).unwrap();
            d.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            d.reset_fences(&[fence]).unwrap();
            let mut ts = [0u64; 2];
            d.get_query_pool_results(qp, 0, &mut ts, vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT).unwrap();
            let us = (ts[1] - ts[0]) as f64 * gpu.period as f64 / 1000.0 / reps.max(1) as f64;
            let out = RgbImage::from_fn(ow, oh, |x, y| {
                let k = ((y * ow + x) * 4) as usize;
                Rgb([*down_ptr.add(k), *down_ptr.add(k + 1), *down_ptr.add(k + 2)])
            });
            let path = format!("{prefix}_m{mode}.png");
            out.save(&path).unwrap();
            println!("mode {mode}: {us:.1} us per draw ({iw}x{ih} -> {ow}x{oh}) {path}");
        }
        let _ = &gpu.instance;
    }
}
