cask "unfudged-staging" do
  version "0.17.8"
  sha256 "887714e6316e4aec609eed56ace32ffc290ceb76899900abd78f5debc81f9f5c"

  url "https://downloads.unfudged.io/staging/v0.17.8/UNFUDGED-v0.17.8-universal.dmg"
  name "UNFUDGED (Staging)"
  desc "High-resolution filesystem flight recorder - staging build"
  homepage "https://unfudged.io"

  conflicts_with cask: "unfudged"

  depends_on formula: "cyrusradfar/unf/unf-staging"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
