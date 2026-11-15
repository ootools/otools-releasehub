use otools_releasehub_native::{otools_plugin_free, otools_plugin_invoke};
#[test]
fn ffi_health_invalid_json_and_null_pointers_are_safe() {
    let mut len = 0;
    assert!(otools_plugin_invoke(std::ptr::null(), 0, &mut len).is_null());
    for input in [
        b"invalid".as_slice(),
        b"{\"method\":\"__otools_health__\"}".as_slice(),
    ] {
        let ptr = otools_plugin_invoke(input.as_ptr(), input.len(), &mut len);
        assert!(!ptr.is_null());
        let response: serde_json::Value =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(ptr, len) }).unwrap();
        if input[0] == b'{' {
            assert_eq!(response["data"]["id"], "otools-releasehub");
        } else {
            assert_eq!(response["ok"], false);
        }
        otools_plugin_free(ptr, len);
    }
    otools_plugin_free(std::ptr::null_mut(), 0);
}
