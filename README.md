bootloader version :0.11

x86 config kernel. 

Way to Setup 
git clone 

cd Kafka-OS

`cargo build -p blog_os --target kernel/x86_64-blog_os.json && cargo run -p runner`

if the build is not getting updated then do 
` cargo clean  && cargo build -p blog_os --target kernel/x86_64-blog_os.json && cargo run -p runner`
