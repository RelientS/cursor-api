use alloc::sync::Arc;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures::stream::Stream;
use tokio::sync::Notify;

/// 可通过外部信号控制 drop 的 Stream 包装器
pub struct DroppableStream<S> {
    stream: Option<S>,
    notify: Arc<Notify>,
    dropped: bool,
}

/// 用于触发 Stream drop 的控制句柄
#[derive(Clone)]
#[repr(transparent)]
pub struct DropHandle {
    notify: Arc<Notify>,
}

impl<S> DroppableStream<S>
where S: Stream + Unpin
{
    /// 创建新的可控制 Stream 和其控制句柄
    pub fn new(stream: S) -> (Self, DropHandle) {
        let notify = Arc::new(Notify::new());

        (
            Self { stream: Some(stream), notify: notify.clone(), dropped: false },
            DropHandle { notify },
        )
    }
}

impl<S> Stream for DroppableStream<S>
where S: Stream + Unpin
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // 如果已经处理过 drop，直接返回
        if this.dropped {
            return Poll::Ready(None);
        }

        // 检查是否有 drop 通知
        let notified = this.notify.notified();
        futures::pin_mut!(notified);

        if notified.poll(cx).is_ready() {
            this.stream = None;
            this.dropped = true;
            return Poll::Ready(None);
        }

        // 轮询内部 stream
        if let Some(ref mut stream) = this.stream {
            Pin::new(stream).poll_next(cx)
        } else {
            Poll::Ready(None)
        }
    }
}

impl DropHandle {
    /// 触发关联 Stream 的 drop
    pub fn drop_stream(self) { self.notify.notify_one(); }
}
