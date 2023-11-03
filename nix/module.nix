pkg:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.mev-rs;

  mev-rs-for-features = { features ? "" }: pkg.mev-rs {
    inherit features;
    inherit (pkgs) system;
  };
  any-components-enabled = cfg.boost.enable || cfg.relay.enable || cfg.build.enable;

  build-cmd = mev-rs: cfg: ''
    ${mev-rs}/bin/mev build \
    node \
    --chain ${cfg.network} \
    --full \
    --http \
    --authrpc.jwtsecret ${cfg.jwt-secret} \
    --mev-builder-config ${cfg.config-file} \
  '';
in
{
  options.services.mev-rs = with lib; with types; {
    config-file = mkOption {
      type = str;
      description = ''
        path to a config file suitable for the `mev-rs` toolkit
      '';
    };
    additional-features = mkOption {
      type = listOf str;
      default = [ ];
      description = ''
        additional Cargo features to include
      '';
    };
    systemd-after-service = mkOption {
      type = str;
      default = "vc.service";
      description = ''
        the systemd unit will be configured to launch after the name of the service provided here
      '';
    };
    boost = {
      enable = mkEnableOption "enable `mev-boost-rs`";
      config-file = mkOption {
        type = str;
        default = cfg.config-file;
        description = ''
          override the `mev-rs` config for this component
        '';
      };
      features = mkOption {
        type = str;
        default = strings.concatStringsSep "," ([ "boost" ] ++ cfg.additional-features);
        description = ''
          feature set (comma-separated) to enable for `cargo` build
        '';
      };
    };
    relay = {
      enable = mkEnableOption "enable `mev-relay-rs`";
      config-file = mkOption {
        type = str;
        default = cfg.config-file;
        description = ''
          override the `mev-rs` config for this component
        '';
      };
      port = mkOption {
        type = port;
        default = 28545;
        description = "port to expose for the API server";
      };
      features = mkOption {
        type = str;
        default = strings.concatStringsSep "," ([ "relay" ] ++ cfg.additional-features);
        description = ''
          feature set (comma-separated) to enable for `cargo` build
        '';
      };
    };
    build = {
      enable = mkEnableOption "enable `mev-build-rs`";
      config-file = mkOption {
        type = str;
        default = cfg.config-file;
        description = ''
          override the `mev-rs` config for this component
        '';
      };
      jwt-secret = mkOption {
        type = str;
        description = ''
          path to the JWT secret used for the Engine API
        '';
      };
      network = mkOption {
        type = str;
        description = ''
          ethereum network the builder targets
        '';
      };
      features = mkOption {
        type = str;
        default = strings.concatStringsSep "," ([ "build" ] ++ cfg.additional-features);
        description = ''
          feature set (comma-separated) to enable for `cargo` build
        '';
      };
    };
  };

  config = with lib; {
    networking.firewall = mkMerge [
      (mkIf cfg.build.enable { allowedTCPPorts = [ 30303 ]; allowedUDPPorts = [ 30303 ]; })
      (mkIf cfg.relay.enable { allowedTCPPorts = [ cfg.relay.port ]; })
    ];

    environment.systemPackages =
      let
        mev-rs = mev-rs-for-features { };
      in
      mkIf any-components-enabled [
        mev-rs
      ];

    systemd.services = {
      mev-boost-rs =
        let
          component = "boost";
          name = "mev-${component}-rs";
          mev-rs = mev-rs-for-features { features = cfg.boost.features; };
          cmd = ''
            ${mev-rs}/bin/mev \
            ${component} \
            ${cfg.boost.config-file}
          '';
        in
        mkIf cfg.boost.enable {
          description = name;
          wantedBy = [ "multi-user.target" ];
          after = [ cfg.systemd-after-service ];
          serviceConfig = {
            ExecStart = cmd;
            Restart = "on-failure";
            RestartSec = "10s";
            SyslogIdentifier = "boost";
          };
        };
      mev-relay-rs =
        let
          component = "relay";
          name = "mev-${component}-rs";
          mev-rs = mev-rs-for-features { features = cfg.relay.features; };
          cmd = ''
            ${mev-rs}/bin/mev \
            ${component} \
            ${cfg.relay.config-file}
          '';
        in
        mkIf cfg.relay.enable {
          description = name;
          wantedBy = [ "multi-user.target" ];
          after = [ cfg.systemd-after-service ];
          serviceConfig = {
            ExecStart = cmd;
            Restart = "on-failure";
            RestartSec = "10s";
            SyslogIdentifier = "relay";
          };
        };
      mev-build-rs =
        let
          component = "build";
          name = "mev-${component}-rs";
          mev-rs = mev-rs-for-features { features = cfg.build.features; };
          cmd = build-cmd mev-rs cfg.build;
        in
        mkIf cfg.build.enable {
          description = name;
          wantedBy = [ "multi-user.target" ];
          after = [ cfg.systemd-after-service ];
          serviceConfig = {
            ExecStart = cmd;
            Restart = "on-failure";
            RestartSec = "10s";
            SyslogIdentifier = "builder";
          };
        };
    };
  };
}
