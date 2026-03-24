cask "unfudged-staging" do
  version "0.17.12"
  sha256 "dc2c327e25c09817808a81aee542b94de92b89a62d55ae4651ce8a06cfc2ffca"

  url "https://downloads.unfudged.io/staging/v0.17.12/UNFUDGED-v0.17.12-universal.dmg"
  name "UNFUDGED (Staging)"
  desc "High-resolution filesystem flight recorder - staging build"
  homepage "https://unfudged.io"

  conflicts_with cask: "unfudged"

  depends_on formula: "cyrusradfar/unf/unf-staging"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
