fontconfig_h_conf = configuration_data()
fontconfig_h_conf.set('CACHE_VERSION', '@0@'.format(cacheversion))
fontconfig_h = configure_file(
  output: 'fontconfig.h',
  input: 'fontconfig.h.in',
  configuration: fontconfig_h_conf,
)
