fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/mirai.ico");
        resource.set("ProductName", "Mirai");
        resource.set("FileDescription", "Mirai privacy browser");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=failed to embed Windows resources: {error}");
        }
    }
}
