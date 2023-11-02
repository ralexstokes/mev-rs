# NOTE: currently doesn't do what we want...
{
  services.mev-rs = {
    build = {
      config-file = "example.config.toml";
      additional-features = [ "config" ];
    };
  };

  boot.isContainer = true;
  system.stateVersion = "23.11";
}
