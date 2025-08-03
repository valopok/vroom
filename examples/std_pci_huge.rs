use std::{env, process};
use vroom::Error;

pub fn main() -> Result<(), Error> {
    let mut args = env::args();
    args.next();

    let pci_addr = match args.next() {
        Some(arg) => arg,
        None => {
            eprintln!("Usage: cargo run --example std_pci_huge <pci bus id>");
            process::exit(1);
        }
    };

    let mut nvme = vroom::new_pci_and_huge(&pci_addr)?;

    let namespace_ids = nvme.namespace_ids();
    let namespace_id = namespace_ids
        .first()
        .expect("No namespaces exist.")
        .to_owned();
    let queue_capacity = nvme
        .controller_information()
        .maximum_queue_entries_supported;
    let logical_block_address = 0;
    let mut io_queue_pair_1 = nvme.create_io_queue_pair(&namespace_id, queue_capacity)?;
    let mut io_queue_pair_2 = nvme.create_io_queue_pair(&namespace_id, queue_capacity)?;

    const TEXT: &'static str = "Hello, world!";

    let source_1 = TEXT.as_bytes();
    io_queue_pair_1.write_copied(&source_1, logical_block_address)?;
    let mut dest_1 = [0u8; TEXT.len()];
    io_queue_pair_1.read_copied(&mut dest_1, logical_block_address)?;

    let source_2 = TEXT.as_bytes();
    io_queue_pair_2.write_copied(&source_2, logical_block_address)?;
    let mut dest_2 = [0u8; TEXT.len()];
    io_queue_pair_2.read_copied(&mut dest_2, logical_block_address)?;

    nvme.delete_io_queue_pair(io_queue_pair_1)?;
    nvme.delete_io_queue_pair(io_queue_pair_2)?;

    println!("-----source_1: {}", std::str::from_utf8(&source_1).unwrap());
    println!("destination_1: {}", std::str::from_utf8(&dest_1).unwrap());
    println!("-----source_2: {}", std::str::from_utf8(&source_2).unwrap());
    println!("destination_2: {}", std::str::from_utf8(&dest_2).unwrap());

    // let max_queues = nvme
    //     .controller_information()
    //     .maximum_number_of_io_queue_pairs;
    // dbg!(max_queues);
    // dbg!(queue_capacity);
    // let mut queues: Vec<IoQueuePair<HugePageAllocator>> = Vec::new();
    // for _ in 0..max_queues {
    //     let queue = nvme.create_io_queue_pair(&namespace_id, queue_capacity)?;
    //     queues.push(queue);
    // }
    // for queue in queues {
    //     nvme.delete_io_queue_pair(queue)?;
    // }

    nvme.shutdown()
}
