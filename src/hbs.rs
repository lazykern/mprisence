use handlebars::handlebars_helper;

handlebars_helper!(eq: |x: str, y: str| x == y);

pub fn new_hbs() -> handlebars::Handlebars<'static> {
    let mut reg = handlebars_misc_helpers::new_hbs();

    reg.register_helper("eq", Box::new(eq));

    reg
}
