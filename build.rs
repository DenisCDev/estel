fn main() {
    #[cfg(windows)]
    winres::WindowsResource::new()
        .set_icon("assets/avatar-icon.ico")
        .compile()
        .expect("Não foi possível incorporar o ícone do Estel");
}
