mod child;

#[path = "aside.rsx"]
mod aside;

#[path = "tf"]
mod scoped {
    mod leaf;
}

mod boxed {
    #[path = "in_box.rsx"]
    mod inner;
}
