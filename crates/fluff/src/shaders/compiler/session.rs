//! Slang sessions
use std::cell::OnceCell;
use std::ffi::CString;
use std::path::Path;

fn get_slang_global_session() -> slang::GlobalSession {
    thread_local! {
        static SESSION: OnceCell<slang::GlobalSession> = OnceCell::new();
    }

    SESSION.with(|s| {
        s.get_or_init(|| slang::GlobalSession::new().expect("Failed to create Slang session"))
            .clone()
    })
}

pub(crate) fn create_session(profile_id: &str, search_paths: &[&Path], macro_definitions: &[(&str, &str)]) -> slang::Session {
    let global_session = get_slang_global_session();

    let mut search_paths_cstr = vec![];
    for path in search_paths {
        search_paths_cstr.push(CString::new(path.to_str().unwrap()).unwrap());
    }
    let search_path_ptrs = search_paths_cstr.iter().map(|p| p.as_ptr()).collect::<Vec<_>>();

    let target_desc = slang::TargetDesc::default().format(slang::CompileTarget::Spirv);
    let targets = [target_desc];

    let profile = global_session.find_profile(profile_id);
    let mut compiler_options = slang::CompilerOptions::default()
        .glsl_force_scalar_layout(true)
        .optimization(slang::OptimizationLevel::Default)
        .profile(profile);

    for (k,v) in macro_definitions {
        compiler_options = compiler_options.macro_define(k,v);
    }
    
    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(&search_path_ptrs)
        .options(&compiler_options);
    

    let session = global_session
        .create_session(&session_desc)
        .expect("failed to create session");
    session
}
