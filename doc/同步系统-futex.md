# futex

futex是Linux提供的用户内核混合的锁操作，相比于自旋锁，线程在尝试获取futex失败后可以原子地睡眠而不用担心线程因临界区问题导致的无法唤醒的问题。

## futex系统调用

futex系统调用的具有如下参数：

| 名字            | 类型             |
| --------------- | ---------------- |
| uaddr           | u32*             |
| futex_op        | OP               |
| val             | u32              |
| time_out / val2 | time_spec* / u32 |
| uaddr2          | u32*             |
| val3            | u32              |

对于每个futex_op：

### FUTEX_PRIVATE_FLAG

这是一个符号位，可以与其他操作标志一同使用。

如果包含此标志位，futex不会和其他进程共享，例如匿名映射上是futex将不会在进程之间生效。

### FUTEX_CLOCK_REALTIME

这是一个符号位，可以与其他操作标志一同使用。

如果包含此标志位，futex的time_out将使用实时时钟，此时钟不会随系统时间的修改而变化。

### FUTEX_WAIT

如果uaddr中的值和val相同则睡眠并等待FUTEX_WAKE唤醒，如果不同则操作失败并返回EAGAIN。

被唤醒后返回0，即使是被定时器唤醒的。

### FUTEX_WAKE

唤醒uaddr的futex上至多val个线程。

返回被唤醒线程的数量。

### FUTEX_FD

创建一个文件描述符并关联到uaddr上的futex。此操作已经被移除。

### FUTEX_REQUEUE

和 FUTEX_CMP_REQUEUE 类似，但不使用val3检查。

返回被唤醒的线程数量。

### FUTEX_CMP_REQUEUE

先检查uaddr上的值是否为val3，如果不相等则返回EAGAIN。如果等于val3则唤醒至多val个在uaddr上futex中等待的线程。如果等待的线程超过val个，将未唤醒的线程转移到uaddr2的等待队列上。val2是转移到uaddr2上futex的线程上限。

如果val为INT_MAX，此操作将等价于FUTEX_WAKE；如果val2为0，此操作将等价于FUTEX_WAIT。

返回被唤醒的线程数量。

### FUTEX_WAKE_OP

使用CAS操作保存uaddr2上的值并按val3规定修改，唤醒uaddr上futex的至多val个线程，根据uaddr2上先前值的结果唤醒uaddr2的futex上至多val2个线程。

val3按高位到低位包含op,cmp,oparg,cmparg四个字段，分别占4,4,12,12bit。

op的定义如下：

* FUTEX_OP_SET:     uaddr2 = oparg
* FUTEX_OP_ADD:    uaddr2 += oparg
* FUTEX_OP_OR:      uaddr2 |= oparg
* FUTEX_OP_ANDN: uaddr2 &= oparg
* FUTEX_OP_XOR:     uaddr2 ^= oparg
* FUTEX_OP_ARG_SHIFT: 此位与上述操作一同使用，包含此位则用1<<oparg作为操作数。

cmp定义如下：

* FUTEX_OP_CMP_EQ 如果oldval == cmparg则唤醒
* FUTEX_OP_CMP_NE 如果oldval != cmparg则唤醒
* FUTEX_OP_CMP_LT 如果oldval < cmparg则唤醒
* FUTEX_OP_CMP_LE 如果oldval <= cmparg则唤醒
* FUTEX_OP_CMP_GT 如果oldval  > cmparg则唤醒
* FUTEX_OP_CMP_GE 如果oldval >= cmparg则唤醒

此操作的返回值是在uaddr和uaddr2上唤醒线程的总和。

### FUTEX_WAIT_BITSET

此操作和FUTEX_WAIT很相似，但val3放置了掩码并至少设置了一个位。掩码将被保存到等待线程的状态中。

### FUTEX_WAKE_BITSET

次操作和FUTEX_WAKE很类似，但val3放置了掩码并至少设置了一个位。唤醒时只要等待线程的状态和val3的与不为0即可唤醒。

FUTEX_BITSET_MATCH_ANY被包含在任何位集合中，FUTEX_WAIT和FUTEX_WAKE等价于BITSET版本的实现并使用FUTEX_BITSET_MATCH_ANY作为参数。

## FUTEX的作用

自旋锁在处理不太耗时的临界区效率很高，但如果临界区需要长时间占用，例如IO时，自旋锁会让大量线程在自旋锁上空转，我们希望能够让线程在等待资源时主动释放CPU。一种朴素的实现如下：

```rust
fn access_critical_zone() {
    while !try_lock() {
        sleep();
    }
    run_other(); // 访问临界区
    unlock();
    notify();
}
```

这个实现是有问题的。考虑两个线程AB以如下顺序访问：

A.try_lock (success) => B.try_lock (false) => A.unlock => A.notify => B.sleep

在这个访问顺序下，由于try_lock和sleep中间存在空隙，notify完全可以在sleep之前进行，于是B线程在睡眠后再也无法被唤醒了。futex是如何解决这个问题的？futex相当于在sleep里增加了判断机制，futex会在锁保护下判断futex地址中的值是否等于用户的输入，如果不相等则直接返回，相等才会真正地进入睡眠。时候futex后，竞争下的睡眠与唤醒有三种情况：

A.try_lock (success) => B.try_lock (false) => B.futex_wait => A.unlock => A.futex_wake

在这种情况下，B可以成功地睡眠，并被A唤醒。

A.try_lock (success) => B.try_lock (false) => A.unlock => B.futex_wait => A.futex_wake

在这种情况下，由于A.unlock会改变内存中的值，B.futex_wait会失败并立刻返回，再次尝试获取锁操作。

A.try_lock (success) => B.try_lock (false) => A.unlock => A.futex_wake => B.futex_wait

在这种情况下，B.futex_wait也会因为A.unlock改变了内存中的值而操作失败并立刻返回，再次尝试获取锁操作。

可见futex可以解决线程睡眠因为存在空隙而导致的线程丢失问题，不会导致线程永远无法唤醒。

## FUTEX实现

futex需要保证操作的原子性。futex的等待操作可以粗略分为内存检查与加入睡眠队列两部分，它需要保证有效的wake执行后不存在仍然睡眠的线程。如果wake操作在内存检查和加入队列之间执行那么futex操作依然会导致线程丢失，因此内存值检查和加入睡眠队列操作必须在锁的保护下进行。

获取锁的开销是非常大的。为了提高效率，futex会在进入系统调用后无锁地检查一次值，如果值已经变化了就立刻返回，如果等于要求值才会进入等待过程。

还有一个问题是线程睡眠队列放在哪里。FTL OS采用了段式页表管理机制，它实际上也可以储存一些状态，例如等待队列。在段管理器中增加一个futex字段，使用map储存地址到futex单元的映射。

进程的映射管理器需要获取锁，因此从它那里直接获取futex对象的效率是非常低的。因此考虑在线程控制块中增加无锁的futex对象索引器，索引器持有Weak指针加速futex访问，映射管理器拥有Arc所有权。索引器使用Weak而不是Arc的原因是munmap后持有所有权的页面管理器会析构并强制线程重新获取新的futex对象，而Arc会导致munmap后线程依然从索引器使用旧的futex对象，而新线程的futex对象都是新创建的。Weak索引器会产生创建和析构的两次原子操作，但futex自身就需要获取锁，如果不使用索引器还需要获取进程对象的锁，考虑竞争因素开销并没有增加。

### FUTEX的并发安全

上述实现的futex索引并不会操作futex的数据，因此没有并发安全问题。只考虑睡眠与唤醒涉及的数据，futex的数据可以划分为队列和等待器，考虑数据的访问，数据操作者可以分为睡眠者和唤醒者。先不考虑超时与队列转移，最简单的futex可以套用睡眠锁模型，唤醒者只需要修改等待器的标志位并唤醒任务即可，睡眠者通过访问标志位可以无锁判断自身是否已经被唤醒。

当考虑超时机制后情况发生了变化，因为此时睡眠者不再由唤醒者唤醒了，而是由自身唤醒。由自身唤醒的数据结构需要修改等待队列来将自身撤销，因此睡眠者需要储存队列锁的位置。

futex还剩下最后一个操作，队列转移。在队列转移下，futex的所有数据之间都具有了顺序意义，设有两个futex对象A和B，以及一个目前在A上等待的等待器X。现在发生了史诗小概率事件：线程U都在尝试将A的队列转移到B，线程V尝试将B的队列转移到A，线程W拥有等待器X，而此时它超时了，如何在这种情况下保证不发生死锁或数据竞争？

解决这个问题非常简单，只需要和早期Linux一样来一个全局大锁将全部操作强行串行化即可，但FTL OS不想放弃治疗。现在进行一些推理：

* 由于队列转移的存在，等待器对应的实际队列的不确定的，因此等待器上保存的队列引用是临界区数据。
* 当从队列A转移X到队列B时，队列A和队列B的锁都会在某时刻被持有，同时等待器上的队列指针会在某时刻改变。

显然两个队列AB都必然包含锁。FTL OS不希望在等待器上也增加一个锁，因为这会导致批量唤醒线程时产生极大量的原子操作，还需要处理更复杂死锁问题。是否可以由队列锁保护等待器上的队列指针的有效性？FTL OS认为这是可行的，因为超时发生时需要将当前节点从队列上撤销，因此获取队列锁是必须的。

这又产生了一个问题，等待器需要从队列指针找到实际的队列再获取对应的锁。这种情况下需要使用双重检查方法，假设队列指针是有效的，再获取对应的锁，再读取队列指针。队列指针同时兼有寻址和确认两个职责。使用这个方法需要保证X移出队列A时队列指针必须在临界区内修改。

#### 是否存在某个阶段同时持有了队列A和队列B的锁？

如果存在，就可以直接在此阶段将等待器的队列指针从A改为B。但这需要处理死锁问题，我们可以规定某种全局性顺序并强制两个锁按顺序获取来避免循环等待，最简单的顺序就是内存的地址。

如果不存在，那么等待器上的指针就存在无效阶段。无效阶段只能在短时间内存在，唤醒者需要保证在短时间内将等待器放置在新的队列上并设置有效的指针。当等待器发现无效指针时需要如自旋锁一样等待，直到获取到一个有效的指针并再次通过双重检查。这个方式可以将两个队列的临界区占有时间都减小一半，提供更高的并发度。为了避免中断导致其他线程长时间空转，队列移动时会关中断。

FTL OS使用不存在同时持有锁的方案。

#### 如何保证队列指针指向了有效队列而不是无效内存？

队列指针使用Arc智能指针并在替换时由RCU释放。但Arc是标准库，只能额外携带一个状态（空指针）。因此唤醒标志需要额外的空间放置。

### 实现细节

#### 加入队列

获取futex，创建等待器，获取futex锁加入队列。

#### 唤醒者唤醒

获取futex，获取队列锁，将待唤醒的任务的队列指针设为“已唤醒”，使用waker。

#### 睡眠者poll

获取队列指针，如果是“已唤醒”则结束睡眠，如果未超时则继续睡眠。当队列指针变为已唤醒后禁止其他相关操作。

超时后如果是“无效”则重新获取，如果是队列指针则从指针获取队列锁，之后再次读取指针判断是否发生变化，如果变化了则释放锁重新获取。以上双重检查通过后释放自身节点。

