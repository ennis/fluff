#[derive(Copy, Clone)]
pub struct TemplateEntry {
    pub name: &'static str,
    // pub builder: fn() -> RcElement,
}

inventory::collect!(TemplateEntry);

#[macro_export]
macro_rules! register_template {
    ($t:ty) => {
        $crate::inventory::submit!($crate::template::TemplateEntry {
            name: stringify!($t),
            builder: || <$t>::default(),
        });
    };
}


