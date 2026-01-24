generate_static_variable!(IS_PREVIEW bool);

fn _initialize() -> bool {
  unsafe { super::cfg_feature::get_unchecked() }.contains("__preview")
}
