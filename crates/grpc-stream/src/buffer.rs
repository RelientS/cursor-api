//! 内部缓冲区管理

use core::iter::FusedIterator;

use bytes::{Buf as _, BytesMut};

use crate::frame::RawMessage;

/// 消息缓冲区（内部使用）
pub struct Buffer {
    inner: BytesMut,
}

impl Buffer {
    #[inline]
    pub fn new() -> Self { Self { inner: BytesMut::new() } }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { inner: BytesMut::with_capacity(capacity) }
    }

    #[inline]
    pub fn len(&self) -> usize { self.inner.len() }

    #[inline]
    pub fn is_empty(&self) -> bool { self.inner.is_empty() }

    #[inline]
    pub fn extend_from_slice(&mut self, data: &[u8]) { self.inner.extend_from_slice(data) }

    #[inline]
    pub fn advance(&mut self, cnt: usize) { self.inner.advance(cnt) }
}

impl Default for Buffer {
    #[inline]
    fn default() -> Self { Self::new() }
}

impl AsRef<[u8]> for Buffer {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.inner.as_ref() }
}

/// 消息迭代器（内部使用）
#[derive(Debug, Clone)]
pub struct MessageIter<'b> {
    buffer: &'b [u8],
    offset: usize,
}

impl<'b> MessageIter<'b> {
    /// 返回当前已消耗的字节数
    #[inline]
    pub fn offset(&self) -> usize { self.offset }
}

impl<'b> Iterator for MessageIter<'b> {
    type Item = RawMessage<'b>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // 至少需要 5 字节（1 字节 type + 4 字节 length）
        if self.offset + 5 > self.buffer.len() {
            return None;
        }

        let r#type = unsafe {
            let ptr: *const u8 =
                ::core::intrinsics::slice_get_unchecked(self.buffer as *const [u8], self.offset);
            *ptr
        };
        let msg_len = u32::from_be_bytes(unsafe {
            *get_offset_len_noubcheck(self.buffer, self.offset + 1, 4).cast()
        }) as usize;

        // 检查消息是否完整
        if self.offset + 5 + msg_len > self.buffer.len() {
            return None;
        }

        self.offset += 5;

        let data = unsafe { &*get_offset_len_noubcheck(self.buffer, self.offset, msg_len) };

        self.offset += msg_len;

        Some(RawMessage { r#type, data })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        // 精确计算剩余完整消息数量
        let mut count = 0;
        let mut offset = self.offset;

        while offset + 5 <= self.buffer.len() {
            let msg_len = u32::from_be_bytes(unsafe {
                *get_offset_len_noubcheck(self.buffer, offset + 1, 4).cast()
            }) as usize;

            if offset + 5 + msg_len > self.buffer.len() {
                break;
            }

            count += 1;
            offset += 5 + msg_len;
        }

        (count, Some(count)) // 精确值
    }
}

// 实现 ExactSizeIterator
impl<'b> ExactSizeIterator for MessageIter<'b> {
    #[inline]
    fn len(&self) -> usize {
        // size_hint() 已经返回精确值，直接使用
        self.size_hint().0
    }
}

// 实现 FusedIterator
impl<'b> FusedIterator for MessageIter<'b> {}

impl<'b> IntoIterator for &'b Buffer {
    type Item = RawMessage<'b>;
    type IntoIter = MessageIter<'b>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter { MessageIter { buffer: self.inner.as_ref(), offset: 0 } }
}

#[inline(always)]
const unsafe fn get_offset_len_noubcheck<T>(
    ptr: *const [T],
    offset: usize,
    len: usize,
) -> *const [T] {
    let ptr = ptr as *const T;
    // SAFETY: The caller already checked these preconditions
    let ptr = unsafe { ::core::intrinsics::offset(ptr, offset) };
    ::core::intrinsics::aggregate_raw_ptr(ptr, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_size_iterator() {
        let mut buffer = Buffer::new();

        // 构造两个消息：type=0, len=3, data="abc"
        buffer.extend_from_slice(&[0, 0, 0, 0, 3, b'a', b'b', b'c']);
        buffer.extend_from_slice(&[0, 0, 0, 0, 2, b'x', b'y']);

        let iter = (&buffer).into_iter();

        // 验证 ExactSizeIterator
        assert_eq!(iter.len(), 2);
        assert_eq!(iter.size_hint(), (2, Some(2)));

        let messages: Vec<_> = iter.collect();
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_fused_iterator() {
        let buffer = Buffer::new(); // 空缓冲区

        let mut iter = (&buffer).into_iter();

        // 验证 FusedIterator
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None); // 仍然是 None
        assert_eq!(iter.next(), None); // 永远是 None
    }

    #[test]
    fn test_clone_iterator() {
        let mut buffer = Buffer::new();
        buffer.extend_from_slice(&[0, 0, 0, 0, 3, b'a', b'b', b'c']);

        let iter = (&buffer).into_iter();
        let iter_clone = iter.clone();

        // 消耗原迭代器
        assert_eq!(iter.count(), 1);

        // 副本仍然可用
        assert_eq!(iter_clone.count(), 1);
    }
}
