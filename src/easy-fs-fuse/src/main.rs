use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    sync::{Mutex, Arc},
};

use easy_fs::{BlockDevice, BLOCK_SIZE, EasyFileSystem};

struct BlockFile(Mutex<File>);

impl BlockDevice for BlockFile {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SIZE) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.read(buf).unwrap(), BLOCK_SIZE, "Not a complete block!");
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SIZE) as u64))
            .expect("Error when seeking!");
        assert_eq!(
            file.write(buf).unwrap(),
            BLOCK_SIZE,
            "Not a complete block!"
        );
    }
}

fn main() {
    println!("Hello, world!");
}

// run `cargo test`
#[test]
fn efs_test() -> std::io::Result<()> {
    // init block device
    let block_file = Arc::new(BlockFile(Mutex::new({
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("target/fs.img")?;
        f.set_len(8192 * 512).unwrap();
        f
    })));
    // init file system, 
    EasyFileSystem::create(block_file.clone(), 4096, 1);
    let efs = EasyFileSystem::open(block_file.clone());
    let root_inode = EasyFileSystem::root_inode(efs.clone());
    // create two files
    println!("listing all files");
    root_inode.create("filea");
    root_inode.create("fileb");
    for name in root_inode.ls() {
        println!("{}", name);
    }
    println!("writing data to filea");
    let filea = root_inode.find("filea").unwrap();
    let greet_str = "Hello, world!";
    filea.write_at(0, greet_str.as_bytes());
    //let mut buffer = [0u8; 512];
    let mut buffer = [0u8; 233];
    let len = filea.read_at(0, &mut buffer);
    assert_eq!(greet_str, core::str::from_utf8(&buffer[..len]).unwrap(),);

    let mut random_str_test = |len: usize| {
        filea.clear();
        assert_eq!(filea.read_at(0, &mut buffer), 0,);
        let mut str = String::new();
        use rand;
        // random digit
        for _ in 0..len {
            str.push(char::from('0' as u8 + rand::random::<u8>() % 10));
        }
        filea.write_at(0, str.as_bytes());
        let mut read_buffer = [0u8; 127];
        let mut offset = 0usize;
        let mut read_str = String::new();
        loop {
            let len = filea.read_at(offset, &mut read_buffer);
            if len == 0 {
                break;
            }
            offset += len;
            read_str.push_str(core::str::from_utf8(&read_buffer[..len]).unwrap());
        }
        assert_eq!(str, read_str);
    };

    println!("executing random_str_test 0");
    random_str_test(4 * BLOCK_SIZE);
    println!("executing random_str_test 1");
    random_str_test(8 * BLOCK_SIZE + BLOCK_SIZE / 2);
    println!("executing random_str_test 2");
    random_str_test(100 * BLOCK_SIZE);
    println!("executing random_str_test 3");
    random_str_test(70 * BLOCK_SIZE + BLOCK_SIZE / 7);
    println!("executing random_str_test 4");
    random_str_test((12 + 128) * BLOCK_SIZE);
    println!("executing random_str_test 5");
    random_str_test(400 * BLOCK_SIZE);
    println!("executing random_str_test 6");
    random_str_test(1000 * BLOCK_SIZE);
    println!("executing random_str_test 7");
    random_str_test(2000 * BLOCK_SIZE);

    Ok(())
}
