use grace::{ShutdownGuard, ShutdownType};

fn main() {
    {
        let (_guard, rx) =
            ShutdownGuard::new_channel(&[ShutdownType::Interrupt, ShutdownType::Terminate]);
        println!("Hello, world!");
        let type_ = rx.recv().unwrap();
        println!("{:?}", type_);
    }
    std::thread::park();
}
