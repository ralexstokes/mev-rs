{
  services.mev-rs = {
    config-file = "example.config.toml";
    additional-features = [ "config" ];
    build = {
      enable = true;
      jwt-secret = "some-path";
      network = "sepolia";
    };
    relay = {
      enable = true;
    };
  };

  boot.isContainer = true;
  system.stateVersion = "23.11";
}
