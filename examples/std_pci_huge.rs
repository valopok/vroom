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
    const LENGTH: usize = TEXT.len();

    let mut source_1 = io_queue_pair_1.allocate_buffer(LENGTH)?;
    let mut dest_1 = io_queue_pair_1.allocate_buffer(LENGTH)?;
    for (i, byte) in TEXT.bytes().enumerate() {
        source_1[i] = byte;
    }
    io_queue_pair_1.write(&source_1, logical_block_address)?;
    io_queue_pair_1.read(&mut dest_1, logical_block_address)?;

    let mut source_2 = io_queue_pair_2.allocate_buffer(TEXT.len())?;
    let mut dest_2 = io_queue_pair_2.allocate_buffer(TEXT.len())?;
    for (i, byte) in TEXT.bytes().enumerate() {
        source_2[i] = byte;
    }
    io_queue_pair_2.write(&source_2, logical_block_address)?;
    io_queue_pair_2.read(&mut dest_2, logical_block_address)?;

    println!("-----source_1: {}", std::str::from_utf8(&source_1[..]).unwrap());
    println!("destination_1: {}", std::str::from_utf8(&dest_1[..]).unwrap());
    println!("-----source_2: {}", std::str::from_utf8(&source_2[..]).unwrap());
    println!("destination_2: {}", std::str::from_utf8(&dest_2[..]).unwrap());

    io_queue_pair_1.deallocate_buffer(source_1)?;
    io_queue_pair_1.deallocate_buffer(dest_1)?;
    io_queue_pair_2.deallocate_buffer(source_2)?;
    io_queue_pair_2.deallocate_buffer(dest_2)?;

    nvme.shutdown(vec![io_queue_pair_1, io_queue_pair_2])
}
