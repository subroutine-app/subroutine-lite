mod protocol;

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

use jni::{
    JNIEnv,
    objects::{JCharArray, JObject, JString},
    sys::jstring,
};
use protocol::{MAX_REQUEST_BYTES, Response};

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_example_subroutine_1simple_ui_creator_NativeItemParser_nativeParse(
    mut env: JNIEnv<'_>,
    _receiver: JObject<'_>,
    request_json: JString<'_>,
) -> jstring {
    let result = catch_unwind(AssertUnwindSafe(|| invoke(&mut env, &request_json)));
    let error = match result {
        Ok(Ok(response)) => return response,
        Ok(Err(error)) => error,
        Err(payload) => {
            // Even a panic payload with a panicking destructor must not cross JNI.
            std::mem::forget(payload);
            "Native parser failed unexpectedly"
        }
    };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !env.exception_check().unwrap_or(true) {
            let _ = env.throw_new("java/lang/IllegalStateException", error);
        }
    }))
    .map_err(std::mem::forget);
    ptr::null_mut()
}

fn invoke(env: &mut JNIEnv<'_>, request: &JString<'_>) -> Result<jstring, &'static str> {
    let response = match read_request(env, request)? {
        Ok(json) => protocol::parse(&json)?,
        Err(reason) => Response::error(reason),
    };
    let json = serde_json::to_string(&response).map_err(|_| "Could not encode parser response")?;
    env.new_string(json)
        .map(JString::into_raw)
        .map_err(|_| "Could not allocate parser response")
}

fn read_request(
    env: &mut JNIEnv<'_>,
    request: &JString<'_>,
) -> Result<Result<String, &'static str>, &'static str> {
    if request.is_null() {
        return Ok(Err("Parser request must not be null"));
    }
    let length = env
        .call_method(request, "length", "()I", &[])
        .and_then(|value| value.i())
        .map_err(|_| "Could not read parser request length")?;
    if length < 0 || length as usize > MAX_REQUEST_BYTES {
        return Ok(Err("Parser request exceeds the 131072-byte limit"));
    }
    // Read UTF-16 explicitly: JNI modified UTF-8 permits unpaired surrogates.
    let chars = env
        .call_method(request, "toCharArray", "()[C", &[])
        .and_then(|value| value.l())
        .map(JCharArray::from)
        .map_err(|_| "Could not read parser request characters")?;
    let mut buffer = vec![0; length as usize];
    env.get_char_array_region(&chars, 0, &mut buffer)
        .map_err(|_| "Could not copy parser request characters")?;
    Ok(String::from_utf16(&buffer).map_err(|_| "Parser request contains invalid Unicode"))
}
