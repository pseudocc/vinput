# Backport the functional changes in NixOS/nixpkgs PR #570230 at 0839ff0.
# Tested against nixpkgs-unstable 3ec636362e1816bc2f0888fa0fa2c69f1bc9e504.
final: prev:
let
  inherit (prev) lib;

  incompatible =
    package: detail:
    throw "vinput NPU overlay: incompatible ${package} (${detail}); rebase PR #570230 for this nixpkgs";

  checkSource =
    package: version: hash: drv:
    if drv.version != version || (drv.src.outputHash or null) != hash then
      incompatible package "expected version ${version} and the locked source hash"
    else
      drv;

  baseOpenvino =
    checkSource "openvino" "2026.4.0" "sha256-WIFbm2/lptoJGBVqpYM/qD9MHOTpayMT95fXOLmlfP4="
      (prev.openvino or (incompatible "openvino" "package not found"));
  baseDriver =
    checkSource "intel-npu-driver" "1.38.0" "sha256-YuxthJBkCY0t/fdJgq5m+z0F4wuLzYLKrJpui99J3ZM="
      (prev.intel-npu-driver or (incompatible "intel-npu-driver" "package not found"));
in
{
  openvino-intel-npu-compiler =
    if prev ? openvino-intel-npu-compiler then
      incompatible "openvino-intel-npu-compiler" "already provided by nixpkgs"
    else
      final.callPackage ./openvino-npu-compiler.nix { };

  openvino = baseOpenvino.overrideAttrs (
    old:
    let
      inputs = old.nativeBuildInputs or [ ];
      isOldHook = input: toString input == toString prev.addDriverRunpath;
      isPatchelfHook = input: toString input == toString prev.autoPatchelfHook;
      oldHookCount = builtins.length (builtins.filter isOldHook inputs);
      patchelfHookCount = builtins.length (builtins.filter isPatchelfHook inputs);
      oldHookIndex = lib.lists.findFirstIndex isOldHook null inputs;
      patchelfHookIndex = lib.lists.findFirstIndex isPatchelfHook null inputs;
      newInputs = map (input: if isOldHook input then final.autoAddDriverRunpath else input) inputs;
    in
    if oldHookCount != 1 || patchelfHookCount != 1 || patchelfHookIndex >= oldHookIndex then
      incompatible "openvino" "expected autoPatchelfHook followed by addDriverRunpath exactly once"
    else
      {
        # Replace in place, keeping autoPatchelfHook before autoAddDriverRunpath.
        nativeBuildInputs = newInputs;
        appendRunpaths =
          (old.appendRunpaths or [ ])
          ++ lib.optionals final.stdenv.hostPlatform.isLinux [ "${lib.getLib final.level-zero}/lib" ];
        meta = old.meta // {
          maintainers = (old.meta.maintainers or [ ]) ++ [ lib.maintainers.ryan4yin ];
        };
      }
  );

  intel-npu-driver = baseDriver.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''

      # Upstream looks next to the driver; load the compiler from the store.
      substituteInPlace umd/level_zero_driver/source/ext/vcl_symbols.hpp \
        --replace-fail 'getDriverLibDir() /' \
          'std::filesystem::path("${lib.getLib final.openvino-intel-npu-compiler}/lib") /'
    '';
  });
}
