#![allow(unsafe_op_in_unsafe_fn)]

// pub(crate) mod arc;

use crate::app::constant::EMPTY_STRING;
use core::borrow::Borrow;
use hashbrown::HashSet;
use manually_init::ManuallyInit;
use parking_lot::Mutex;

// 全局实例
static STATIC_POOL: ManuallyInit<Mutex<StaticPool>> = ManuallyInit::new();

#[forbid(unused)]
pub fn init_pool() {
    STATIC_POOL.init(Default::default());
    interned::init();
}

// 静态字符串池
#[derive(Default)]
#[repr(transparent)]
struct StaticPool {
    pool: HashSet<&'static str, ahash::RandomState>,
}

impl StaticPool {
    /// 手动分配内存并复制字符串
    ///
    /// # Safety
    /// 分配的内存会被转换为 'static 生命周期，调用者必须确保不会手动释放
    #[inline]
    unsafe fn alloc_str(s: &str) -> &'static str {
        let len = s.len();
        if len == 0 {
            return EMPTY_STRING;
        }

        // 计算布局，字符串不需要特殊对齐
        let layout = __unwrap!(::core::alloc::Layout::array::<u8>(len));

        // 分配内存
        let ptr = alloc::alloc::alloc(layout);
        if ptr.is_null() {
            // 内存分配失败
            __cold_path!();
            alloc::alloc::handle_alloc_error(layout);
        }

        // 复制字符串内容
        ::core::ptr::copy_nonoverlapping(s.as_ptr(), ptr, len);

        // 从原始部分构造字符串切片
        ::core::str::from_utf8_unchecked(::core::slice::from_raw_parts(ptr, len))
    }

    fn intern(&mut self, s: &str) -> &'static str {
        if let Some(&interned) = self.pool.get(s) {
            interned
        } else {
            // SAFETY: 分配的内存永远不会被释放，因为我们将其存储在静态池中
            let leaked: &'static str = unsafe { Self::alloc_str(s) };
            self.pool.insert(leaked);
            leaked
        }
    }
}

// 公共API
pub fn intern_static<S: Borrow<str>>(s: S) -> &'static str {
    STATIC_POOL.get().lock().intern(s.borrow())
}

// /// 创建带自动注销功能的Arc字符串
// ///
// /// 这个函数使用自定义的ArcStr类型，具有以下优势：
// /// - 自动引用计数管理
// /// - 当引用计数为0时自动从池中移除
// /// - 完全的线程安全
// pub fn intern_arc<S: Borrow<str>>(s: S) -> ArcStr { ArcStr::new(s.borrow()) }
