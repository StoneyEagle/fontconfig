fontations = get_option('fontations')

if (fontations.enabled())
  rust = import('rust')
  cargo = rust.workspace()

  generated_fontconfig = rust.bindgen(
    input: fontconfig_h,
    output: 'fontconfig.rs',
    include_directories: incbase,
    args: [
      '--merge-extern-blocks',
      '--allowlist-item=(FcCharSet.*|FC_(SLANT|WEIGHT|WIDTH)_.*|FcFontSet(Add|Create|Destroy).*|FcLangSet(Create|Destroy|Copy|Add|HasLang)|FcWeightFromOpenType.*|FC_SPACING_.*|FC_FAMILY_.*)',
      '--raw-line=#![allow(nonstandard_style,unused,unnecessary_transmutes)]',
    ],
    # FC_NO_MT=1 is added here to reduce required headers in bindings generation.
    # It does not mean a functional change in multi-threading behavior.
    c_args: ['-DBINDGEN_IGNORE_VISIBILITY=1', '-DFC_NO_MT=1'],
  )

  generated_fcint = rust.bindgen(
    input: ['../src/fcint.h', fclang_h],
    output: 'fcint.rs',
    include_directories: incbase,
    args: [
      '--merge-extern-blocks',
      '--allowlist-item=(FcPattern.*|FcRange.*|FC_.*_OBJECT|FcCharSet.*|FcLangSetFromCharSet|FcGenericAliasGetClassification)',
      '--blocklist-type=(FcCharSet|FcLangSet)',
      '--raw-line=#![allow(nonstandard_style,unused,unnecessary_transmutes)]',
      '--raw-line=extern crate fontconfig_bindings;',
      '--raw-line=pub use fontconfig_bindings::FcCharSet; pub use fontconfig_bindings::FcLangSet;',
    ],
    # FC_NO_MT=1 is added here to reduce required headers in bindings generation.
    # It does not mean a functional change in multi-threading behavior.
    c_args: ['-DBINDGEN_IGNORE_VISIBILITY=1', '-DFC_NO_MT=1'],
  )

  fontconfig_bindings_pkg = cargo.package('fontconfig-bindings')
  fontconfig_bindings_lib = fontconfig_bindings_pkg.library(
    structured_sources(generated_fontconfig))
  fontconfig_bindings_pkg.override_dependency(
    declare_dependency(link_with: fontconfig_bindings_lib))

  fcint_bindings_pkg = cargo.package('fcint-bindings')
  fcint_bindings_lib = fcint_bindings_pkg.library(
    structured_sources(generated_fcint))
  fcint_bindings_pkg.override_dependency(
    declare_dependency(link_with: fcint_bindings_lib))

  fontations_query_sources = ['../src/fcfontations.c', fcstdint_h, fclang_h, alias_headers]
  fontations_query_deps = []
  if woff2_dep.found()
    fontations_query_sources += ['fc_woff2.cpp', 'fc_woff2.h']
    fontations_query_deps += [woff2_dep]
  endif

  fontations_query_lib = static_library(
    'fc_fontations_query',
    include_directories: incbase,
    sources: fontations_query_sources,
    dependencies: fontations_query_deps,
  )

  rust_args_list = ['--check-cfg', 'cfg(skip_external_fonts)', '--check-cfg', 'cfg(have_woff2)']
  if not get_option('tests-external-fonts').allowed()
    rust_args_list += ['--cfg', 'skip_external_fonts']
  endif
  if woff2_dep.found()
    rust_args_list += ['--cfg', 'have_woff2']
  endif

  fc_fontations = cargo.package().library(
    link_with: [pattern_lib, fontations_query_lib],
    install: true,
    rust_args: rust_args_list,
  )
endif
