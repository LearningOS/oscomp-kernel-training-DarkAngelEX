# SD卡驱动的结构:

|   文件   |   实现   |
| :-------:|:--------:|
|layout.rs | 内存映像抽象及通信|
|  mod.rs  | SPIActions接口定义|
| registers.rs | spi控制寄存器抽象|

按照registers.rs,mod.rs,layout.rs依次介绍

## 1. registers.rs

首先定义一个通用的寄存器结构体Reg

```rust
pub struct Reg<T: Sized + Clone + Copy, U> {
    value: T,
    p: PhantomData<U>,
}
```

然后实现new,以及一些读写的基本操作

```rust
impl<T: Sized + Clone + Copy, U> Reg<T, U> {
    pub fn new(initval: T) -> Self {
        Self {
            value: initval,
            p: PhantomData {},
        }
    }
}

impl<T: Sized + Clone + Copy, U> Reg<T, U> {
    pub fn read(&self) -> T {
        let ptr: *const T = &self.value;
        unsafe { ptr.read_volatile() }
    }
    pub fn write(&mut self, val: T) {
        let ptr: *mut T = &mut self.value;
        unsafe {
            ptr.write_volatile(val);
        }
    }
}
```

随后逐个实现SPI协议中控制寄存器的实例,有关其中具体的值可以参考文档
[SD卡中的SPI协议控制寄存器](https://sifive.cdn.prismic.io/sifive/1a82e600-1f93-4f41-b2d8-86ed8b16acba_fu740-c000-manual-v1p6.pdf)
中的第19章

这里仅列出寄存器的相关功能(按照registers中寄存器的实现顺序):
### SCKDIV:
控制串行时钟的频率

### SCKMODE:
控制数据采样和切换数据和时钟上升下降沿的关系

### CSID:
片选寄存器,实现SD卡的选择,这里由于只有一块SD卡,只实现了寄存器的reset

### CSDEF:
设定片选线

### CSMODE:
设置片选模式
1.AUTO：使CS生效或者失效在帧的开始或结束阶段
2.HOLD: 保持CS在初始帧之后一直有效
3.OFF：使得硬件失去对CSpin的掌控

### DELAY0:
cssck字段：控制CS有效和SCK第一次上升沿之间的时延
sckcs字段：控制SCK最后的下降沿和CS失效之间的时延

### DELAY1:
intercs字段：控制最小CS失效时间
interxfr字段：控制两个连续帧在不丢弃CS的情况下之间的延迟，只有在sckmod寄存器是HOLD或者OFF模式下才有用

### FMT:
设置协议，大小端和方向等，传输数据的长度

### TXDATA:
data字段：存储了要传输的一个字节数据，这个数据是被写入FIFO的，注意大小端
full字段：表示FIFO是否已满，如果已经满了，则忽略写到tx_data的数据，这些数据自然也就无法写入FIFO

### rxdata:
data字段：从FIFO中接收的一个字节数据，注意大小端
empty字段：当empty位被设置时，无法从FIFO中获取有效帧/数据

### txmark:
决定传输的FIFO的中断在低于多少阈值下进行触发
txmark字段:当FIFO中的数据少于设置阈值时会触发中断，导致txmark向FIFO中写入数据

### rxmark:
决定接收的FIFO的中断在高于多少阈值时触发
rxmark字段：当FIFO中的数据超出阈值时会从FIFO中读取数据

### fctrl:
控制memory-mapped和programmed-I/O两种模式的切换

### ffmt:
定义指令的一些格式例如指令协议，地址长度等等

### ie:
txwm字段：当FIFO中的数据少于txmark中设定的阈值时，txwm被设置
rxwm字段：当FIFO中的数据多余rxmark中设定的阈值时，rxwm被设置

### ip:
txwm悬挂字段:当FIFO中有充足的数据被写入并且超过了txmark时，txwm的悬挂位被清除
rxwm悬挂字段:当FIFO中有充足的数据被读出并且少于rxmark时，rxwm的悬挂位被清除

## 2. mod.rs中的SPIActions
```rust
pub trait SPIActions {
    fn init(&mut self);
    fn configure(
        &mut self,
        use_lines: u8,       // SPI data line width, 1,2,4 allowed
        data_bit_length: u8, // bits per word, basically 8
        msb_first: bool,     // endianness
    );
    fn switch_cs(&mut self, enable: bool, csid: u32);
    fn set_clk_rate(&mut self, spi_clk: usize);
    fn send_data(&mut self, chip_select: u32, tx: &[u8]);
    fn recv_data(&mut self, chip_select: u32, rx: &mut [u8]);
}
```

一个接口，用于实现SPI协议的一些动作：
init:初始化
switch_cs:设置片选
set_clk_rate:设置时钟频率
send_data:发送数据
recv_data:接收数据

## 3. layout.rs中的内存映像以及SPI协议通信
这一部分就是实现SPI设备的内存映像以及实现如何通信
具体如何通信可以参考Technical Commitee SD Card Association发布的SD Specifications的第7章


### SPI设备的三种实例

```rust
pub enum SPIDevice {
    QSPI0,
    QSPI1,
    QSPI2,
    Other(usize),
}
```

这三种不同的实例分别对应了SPI设备在内存中的不同起始位置:

```rust
impl SPIDevice {
    fn base_addr(&self) -> PhyAddr<RegisterBlock> {
        let a = match self {
            SPIDevice::QSPI0 => 0x10040000usize,
            SPIDevice::QSPI1 => 0x10041000usize,
            SPIDevice::QSPI2 => 0x10050000usize,
            SPIDevice::Other(val) => val.clone(),
        };
        PhyAddr::from_usize(a)
    }
}
```

RegisterBlock就是利用了之前registers中实现的寄存器定义的一个结构体,也就是SPI协议控制器块

### SPIImpl结构体
用SPIImpl结构体实现对SPIDevice的进一步封装，这一层封装主要实现了数据的收发，以及在中断悬挂位没有挂起时的循环等待

```rust
pub struct SPIImpl {
    spi: SPIDevice,
}
impl SPIImpl {
    fn tx_fill(&mut self, data: u8, mut n: usize) {
        while n != 0 && !self.spi.txdata.is_full() {
            self.spi.txdata.write(data as u32);
            n -= 1;
        }
    }
    fn tx_enque(&mut self, data: u8) {
        debug_assert!(!self.spi.txdata.is_full());
        self.spi.txdata.write(data as u32);
    }
    fn rx_deque(&mut self) -> u8 {
        match self.spi.rxdata.flag_read() {
            (false, result) => return result,
            (true, _) => panic!(),
        }
    }
    // 返回可以取出的最大数据数量
    fn rx_wait(&self) {
        while !self.spi.ip.receive_pending() {
            // loop
        }
    }
    // 返回可以发送的最大数据数量
    fn tx_wait(&self) {
        while !self.spi.ip.transmit_pending() {
            // loop
        }
    }
}
```

### 实现SPIActions的接口
#### init:
设置csdef，中断使能寄存器ie，水位寄存器，延迟寄存器，fcrl控制寄存器

#### configure:
设置帧格式寄存器，sckmode寄存器

#### switch_cs:
设置片选模式寄存器csmode，片选id寄存器csid

#### set_clk_rate:
设置时钟频率寄存器sckdiv

#### recv_data:
接收逻辑如下：
* 1.通过fmt寄存器设置方向为接收方向
* 2.设置对应的片选csid
* 3.对FIFO中的数据迭代:
    由于spi一定是全双工的，所以在读取一个字节时要发送无用信息
    设置接收水位寄存器
    等待中断
    接收数据

#### send_data:
发送逻辑如下：
* 1.通过fmt寄存器设置方向为发送方向
* 2.设置对应的片选csid
* 3.对FIFO中的数据迭代:
    通过FU740文档可知，在传输数据时接收线是不被激活的，所以这里不需要也接收数据
    设置发送水位寄存器
    等待中断
    发送数据