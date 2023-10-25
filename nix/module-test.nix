{
  services.mev-rs = {
    enable = "build";
    config-file = "example.config.toml";
    features = "build,config";
  };

  boot.isContainer = true;
  system.stateVersion = "23.11";
}
