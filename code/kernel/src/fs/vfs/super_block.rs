pub struct VfsSuperBlock {
    dev: u32,             // 设备标识符
    block_size_bits: u32, // 块大小的log2
    block_size: usize,    // 块字节数
    max_bytes: usize,     // 允许的最大文件大小
}

trait FsSuperBlock: Send + Sync + 'static {

}
