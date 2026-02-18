cask "unfudged-staging" do
  version "0.16.2"
  sha256 "914da82980687ff7b766bc9b858653204cb92f5f637fbb6f876ffd1536e761fa"

  url "https://github.com/cyrusradfar/homebrew-unf/releases/download/staging-v0.16.2/UNFUDGED-v0.16.2-universal.dmg"
  name "UNFUDGED (Staging)"
  desc "High-resolution filesystem flight recorder - staging build"
  homepage "https://github.com/cyrusradfar/unfudged"

  conflicts_with cask: "unfudged"

  depends_on formula: "cyrusradfar/unf/unf-staging"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end