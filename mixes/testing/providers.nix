# Provider-management fixture: replace only the selected entry and artifact.
{ base, artifact, provider }:
base // {
  entry = base.entry // { inherit provider; };
  artifacts = [ artifact ];
}
