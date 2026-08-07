/// Filter lists vendored into the binary so blocking works offline from the
/// first launch, with no fetch that could itself leak startup telemetry.
/// Snapshots of EasyList and EasyPrivacy (GPLv3+/CC BY-SA dual licensed);
/// refresh by re-downloading into `assets/filter-lists/`.
static EASYLIST: &str = include_str!("../../../assets/filter-lists/easylist.txt");
static EASYPRIVACY: &str = include_str!("../../../assets/filter-lists/easyprivacy.txt");

/// The filter lists bundled with the browser.
pub fn default_filter_lists() -> impl Iterator<Item = &'static str> {
    [EASYLIST, EASYPRIVACY].into_iter()
}
