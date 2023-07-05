use handlebars::handlebars_helper;

handlebars_helper!(eq: |x: str, y: str| x == y);

lazy_static::lazy_static! {
    pub static ref HANDLEBARS: handlebars::Handlebars<'static> = new_hbs();
}

pub fn new_hbs() -> handlebars::Handlebars<'static> {
    let mut reg = handlebars_misc_helpers::new_hbs();

    reg.register_helper("eq", Box::new(eq));
    reg.set_strict_mode(false);

    reg
}
