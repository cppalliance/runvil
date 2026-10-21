//! Tests for the C contract's layout and reserved numbers: the sizes and
//! offsets the fixture mirrors, and the values no version may renumber.

#[cfg(target_pointer_width = "64")]
use std::mem::{offset_of, size_of};

use super::{
    RV_API_MAJOR, RV_API_MINOR, RV_ERR_MISUSE, RV_ERR_REFUSED, RV_ERR_UNSUPPORTED, RV_OK,
    RV_PLUGIN_LOAD, RV_PLUGIN_UNLOAD,
};
#[cfg(target_pointer_width = "64")]
use super::{RvCall, RvHost, RvPluginInfo, RvStr};

#[test]
#[cfg(target_pointer_width = "64")]
fn a_string_is_a_pointer_then_a_length() {
    assert_eq!(size_of::<RvStr>(), 16);
    assert_eq!(offset_of!(RvStr, ptr), 0);
    assert_eq!(offset_of!(RvStr, len), 8);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_plugin_info_keeps_the_layout_the_fixture_mirrors() {
    assert_eq!(size_of::<RvPluginInfo>(), 40);
    assert_eq!(offset_of!(RvPluginInfo, size), 0);
    assert_eq!(offset_of!(RvPluginInfo, reserved), 4);
    assert_eq!(offset_of!(RvPluginInfo, name), 8);
    assert_eq!(offset_of!(RvPluginInfo, version), 24);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_host_table_keeps_the_layout_the_fixture_mirrors() {
    assert_eq!(size_of::<RvHost>(), 32);
    assert_eq!(offset_of!(RvHost, size), 0);
    assert_eq!(offset_of!(RvHost, api_major), 4);
    assert_eq!(offset_of!(RvHost, api_minor), 8);
    assert_eq!(offset_of!(RvHost, reserved), 12);
    assert_eq!(offset_of!(RvHost, refuse), 16);
    assert_eq!(offset_of!(RvHost, describe_plugin), 24);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_call_keeps_the_layout_the_fixture_mirrors() {
    assert_eq!(size_of::<RvCall>(), 40);
    assert_eq!(offset_of!(RvCall, size), 0);
    assert_eq!(offset_of!(RvCall, verb), 4);
    assert_eq!(offset_of!(RvCall, host), 8);
    assert_eq!(offset_of!(RvCall, plugin), 16);
    assert_eq!(offset_of!(RvCall, pack_run), 24);
    assert_eq!(offset_of!(RvCall, args), 32);
}

#[test]
fn the_status_codes_keep_their_numbers() {
    assert_eq!(RV_OK, 0);
    assert_eq!(RV_ERR_MISUSE, 4);
    assert_eq!(RV_ERR_REFUSED, 5);
    assert_eq!(RV_ERR_UNSUPPORTED, 6);
}

#[test]
fn the_verbs_and_the_version_keep_their_numbers() {
    assert_eq!(RV_PLUGIN_LOAD, 1);
    assert_eq!(RV_PLUGIN_UNLOAD, 2);
    assert_eq!(RV_API_MAJOR, 0);
    assert_eq!(RV_API_MINOR, 1);
}
