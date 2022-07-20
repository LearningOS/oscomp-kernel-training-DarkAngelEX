/// 通过类型B减去偏移量访问A
///
/// A: 基类类型 B: 成员类型
pub trait ListAccess<A, B>: 'static {
    fn offset() -> usize;
    #[inline(always)]
    unsafe fn get(b: &B) -> &A {
        &*(b as *const B).cast::<u8>().sub(Self::offset()).cast()
    }
    #[inline(always)]
    unsafe fn get_mut(b: &mut B) -> &mut A {
        &mut *(b as *mut B).cast::<u8>().sub(Self::offset()).cast()
    }
}

fn _access_example() {
    use super::intrusive::InListNode;

    crate::inlist_access!(AccessA, A, node);
    struct A {
        _1: usize,
        node: InListNode<A, AccessA>,
        _2: usize,
    }
    
    let mut a: A = unsafe { core::mem::zeroed() };
    let node = &mut a.node;
    let _1 = &mut a._1;
    let _ta = unsafe { node.access_mut() };
    drop(_1);
    drop(_ta);
}
