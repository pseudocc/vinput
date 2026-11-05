{
  lib,
  rustPlatform,
  makeWrapper,
  alsa-utils,
  curl,
  ydotool,
}:

rustPlatform.buildRustPackage {
  pname = "vinput";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).workspace.package.version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      lib.cleanSourceFilter path type && baseNameOf path != "target";
  };
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];
  postInstall = ''
    wrapProgram "$out/bin/vinput" \
      --prefix PATH : ${lib.makeBinPath [ alsa-utils curl ydotool ]}
  '';

  meta = {
    description = "Voice to text input tool";
    mainProgram = "vinput";
    platforms = lib.platforms.linux;
  };
}
