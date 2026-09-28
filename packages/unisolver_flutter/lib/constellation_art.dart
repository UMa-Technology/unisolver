/// Illustration sets for the constellation art layer (`AnnotateOptionsDto.constellationArt`).
/// Files are named by IAU abbreviation, as `ConstellationAnnotationDto.abbr`; each set's
/// NOTICE.txt and `dataAttributions()` give its author and license.
enum ConstellationArtSet {
  /// Painted, CC BY-SA 4.0. Bundled with the plugin.
  westernNew('packages/unisolver_flutter/assets/art/western_new'),

  /// Low-poly, Free Art License 1.3. Optional: declare
  /// `packages/unisolver_flutter/optional/art/western/` in your app's assets.
  western('packages/unisolver_flutter/optional/art/western');

  const ConstellationArtSet(this.dir);

  /// Asset directory
  final String dir;

  /// Asset key of a constellation's illustration (`Ori` → `…/Ori.webp`); load it with
  /// `rootBundle`
  String assetFor(String abbr) => '$dir/$abbr.webp';
}
