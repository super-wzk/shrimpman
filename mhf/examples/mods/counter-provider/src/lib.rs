use example_counter_sdk::{CounterApi, CounterTable, INTERFACE_ID, Snapshot};
use mhf_mod_sdk::{Host, Mod, Result, abi as api, export_mod};
use std::{
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};

struct CounterState(AtomicU32);
struct CounterMod<'host> {
    host: Host<'host>,
    table: Rc<CounterTable>,
}
impl<'host> Mod<'host> for CounterMod<'host> {
    fn create(host: Host<'host>) -> Result<Self> {
        Ok(Self {
            host,
            table: Rc::new(Box::new(CounterState(AtomicU32::new(0))).into()),
        })
    }
    fn attach(&mut self) -> Result<()> {
        // 函数表独立分配，后续生命周期的 &mut Mod 不独占它；消费者借用，提供方持有 Rc。
        unsafe {
            mhf_mod_sdk::host::register_interface(
                self.host,
                INTERFACE_ID,
                Rc::as_ptr(&self.table).cast(),
            )
        }
    }
}
impl CounterApi for CounterState {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            count: self.0.load(Ordering::Relaxed),
        }
    }
    fn add(&self, amount: u32) -> api::Status {
        // 检查与写入处于同一次原子更新中；溢出返回失败且计数保持不变。
        match self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(amount)
            }) {
            Ok(_) => api::OK,
            Err(_) => api::ERROR,
        }
    }
}
export_mod!(CounterMod);
