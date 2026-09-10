use example_counter_sdk::Counter;
use mhf_mod_sdk::{Host, LogLevel, Mod, Result, export_mod};

struct ConsumerMod<'host> {
    host: Host<'host>,
    counter: Option<Counter<'host>>,
}

impl<'host> Mod<'host> for ConsumerMod<'host> {
    fn create(host: Host<'host>) -> Result<Self> {
        Ok(Self {
            host,
            counter: None,
        })
    }

    fn attach(&mut self) -> Result<()> {
        let counter = Counter::bind(self.host.dependencies())?;
        counter.add(1)?;
        let snapshot = counter.snapshot();
        self.host.log(
            LogLevel::Info,
            &format!("Rust consumer: counter = {}", snapshot.count),
        );
        self.counter = Some(counter);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.counter = None;
        Ok(())
    }
}

export_mod!(ConsumerMod);
