cask "unfudged-staging" do
  version "0.17.9"
  sha256 "65b7c2759ba596040277b75071e2dd630aa8e723dfb869ca61e26151460b5af9"

  url "https://downloads.unfudged.io/staging/v0.17.9/UNFUDGED-v0.17.9-universal.dmg"
  name "UNFUDGED (Staging)"
  desc "High-resolution filesystem flight recorder - staging build"
  homepage "https://unfudged.io"
  license "MIT OR Apache-2.0"

  conflicts_with cask: "unfudged"

  depends_on formula: "cyrusradfar/unf/unf-staging"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
