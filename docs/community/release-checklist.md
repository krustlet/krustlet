# Release Checklist

> If your experience deviates from this document, please document the changes to keep it up-to-date.

This guide will guide you (redundancy added for purposes of punnery) on your journey to release a
new version of Krustlet.

All releases will be of the form `vX.Y.Z` where `X` is the major version number, `Y` is the minor
version number and `Z` is the patch release number. This project strictly follows [semantic
versioning](https://semver.org/) so following this step is critical.

It is important to note that this document assumes that the git remote in your repository that
corresponds to <https://github.com/deislabs/krustlet> is named `upstream`. If yours is not (for
example, if you've chosen to name it `origin` or something similar instead), be sure to adjust the
listed snippets for your local environment accordingly.

If you are not sure what your upstream remote is named, use a command like `git remote -v` to find
out.

If you don't have an upstream remote, you can add one easily using the following command:

```console
git remote add upstream git@github.com:deislabs/krustlet
```

We are also going to be adding security and verification of the release process by providing
signature files. We perform this using [GitHub and
GPG](https://help.github.com/en/github/authenticating-to-github/about-commit-signature-verification).

If you do not have GPG already setup you can follow these steps:

1. [Install the GNU privacy Guard (GPG)](https://gnupg.org)
1. [Generate a new GPG
   key](https://help.github.com/en/github/authenticating-to-github/generating-a-new-gpg-key)
1. [Add your key to your GitHub
   account](https://help.github.com/en/github/authenticating-to-github/adding-a-new-gpg-key-to-your-github-account)
1. [Set your signing key in
   Git](https://help.github.com/en/github/authenticating-to-github/telling-git-about-your-signing-key)

## Step 0: determine what type of release you are cutting

Major releases are for new feature additions and behavioral changes that break backwards
compatibility. Minor releases are for new feature additions that do not break backwards
compatibility. Patch releases are for bug fixes that do not introduce any new features.

There are two versions of the checklist: one for patch releases, and one for major/minor releases.
Follow the checklist that best applies below.

## Checklist for major and minor releases

Follow this checklist if you are cutting a major release (vX.0.0) or a minor release (vX.Y.0):

1. set up your environment
1. increment the version number
1. finalize the release and write the release notes

In this checklist, we are going to reference a few environment variables which you will want to set
for convenience.

For major and minor releases, set the following environment variables, changing the values of
`$MAJOR_RELEASE_NUMBER`, `$MINOR_RELEASE_NUMBER`, and `$RELEASE_CANDIDATE_NUMBER` accordingly:

```console
export MAJOR_RELEASE_NUMBER="0"
export MINOR_RELEASE_NUMBER="1"
export RELEASE_NAME="v${MAJOR_RELEASE_NUMBER}.${MINOR_RELEASE_NUMBER}.0"
```

Now that we know we are going to cut a major or a minor release, we will need to bump the project's
Cargo.toml as well as any crates that have been updated this release.

Open a new pull request against krustlet, bumping the version fields in Cargo.toml:

- <https://github.com/deislabs/krustlet/blob/master/Cargo.toml#L3>

If applicable, in that same pull request, bump the version fields for the `kubelet` and
`oci-distribution` crates:

- <https://github.com/deislabs/krustlet/blob/master/crates/kubelet/Cargo.toml#L3>
- <https://github.com/deislabs/krustlet/blob/master/crates/oci-distribution/Cargo.toml#L3>

Use the following commands to create the commit:

```console
git add .
git commit --gpg-sign -m "bump version to $RELEASE_NAME"
```

Wait until the pull request has been merged into master before proceeding.

After the pull request has been merged, create a new tag from upstream's `master` branch.

```console
git fetch upstream master
git checkout upstream/master
git tag --sign --annotate "${RELEASE_NAME}" --message "Krustlet release ${RELEASE_NAME}"
```

Double-check one last time to make sure everything is in order, then finally push the release tag.

```console
git push upstream $RELEASE_NAME
```

It is usually more beneficial to the end-user if the release notes are hand-written by a human
being/marketing team/dog, so we'll go ahead and write up the release notes.

If you're releasing a major/minor release, listing notable user-facing features is usually
sufficient. For patch releases, do the same, but make note of the symptoms causing the original
issue, who may be affected, and how the patch mitigates the issue.

it should look like this:

```markdown
## Krustlet vX.Y.Z

Krustlet vX.Y.Z is a feature release. This release, we focused on <insert focal point here>. Users are encouraged to upgrade for the best experience.

## Notable features/changes

- TLS bootstrapping support has been added
- Improved error handling for the Kubelet API

## Breaking changes

No known breaking changes were introduced this release.

## Known issues/missing features

- Needs more cowbell
- I gotta have more cowbell!

## Installation

Download Krustlet vX.Y.Z:

- [checksums-vX.Y.Z.txt](https://krustlet.blob.core.windows.net/releases/checksums-vX.Y.Z.txt)
- [krustlet-vX.Y.Z-linux-amd64.tar.gz](https://krustlet.blob.core.windows.net/releases/krustlet-vX.Y.Z-linux-amd64.tar.gz)
- [krustlet-vX.Y.Z-macos-amd64.tar.gz](https://krustlet.blob.core.windows.net/releases/krustlet-vX.Y.Z-macos-amd64.tar.gz)
```

Feel free to bring in your own personality into the release notes; it's nice for people to think
we're not all robots. :)

Double check the URLs are correct. Once finished, go into GitHub and edit the release notes for the
tagged release with the notes written here.

It is now worth getting other people to take a look at the release notes before the release is
published. It is always beneficial as it can be easy to miss something.

For pre-v1.0 releases, make sure to check the checkbox that says "This is a pre-release" to notify
users that this release is identified as non-production ready.

When you are ready to go, hit `publish`, and you're done.

## Checklist for patch releases

Follow this checklist if you are cutting a patch release (vX.Y.Z). The process is largely the same
as cutting a major/minor release, but with a few differences.

1. set up your environment
1. increment the version number
1. cherry-pick fixes
1. finalize the release and write the release notes

In this checklist, we are going to reference a few environment variables which you will want to set
for convenience.

For patch releases, set the following environment variables:

```console
export MAJOR_RELEASE_NUMBER="0"
export MINOR_RELEASE_NUMBER="1"
export PATCH_RELEASE_NUMBER="1"
export PREVIOUS_PATCH_RELEASE_NUMBER="0"
export RELEASE_NAME="v${MAJOR_RELEASE_NUMBER}.${MINOR_RELEASE_NUMBER}.${PATCH_RELEASE_NUMBER}"
export PREVIOUS_RELEASE_NAME="v${MAJOR_RELEASE_NUMBER}.${MINOR_RELEASE_NUMBER}.${PREVIOUS_PATCH_RELEASE_NUMBER}"
export RELEASE_BRANCH_NAME="release-${MAJOR_RELEASE_NUMBER}.${MINOR_RELEASE_NUMBER}.${PATCH_RELEASE_NUMBER}"
```

Now that we know we are going to cut a patch release, we will need to bump the project's Cargo.toml
as well as any crates that have been updated this release.

Open a new pull request against krustlet, bumping the version fields in Cargo.toml:

- <https://github.com/deislabs/krustlet/blob/master/Cargo.toml#L3>

If applicable, in that same pull request, bump the version fields for the `kubelet` and
`oci-distribution` crates:

- <https://github.com/deislabs/krustlet/blob/master/crates/kubelet/Cargo.toml#L3>
- <https://github.com/deislabs/krustlet/blob/master/crates/oci-distribution/Cargo.toml#L3>

Use the following commands to create the commit:

```console
git add .
git commit --gpg-sign -m "bump version to $RELEASE_NAME"
```

Wait until the pull request has been merged into `master` before proceeding.

After the pull request has been merged, checkout the previous tag from upstream and use it to create
a new branch:

```console
git fetch upstream --tags
git checkout $PREVIOUS_RELEASE_NAME
git checkout -b $RELEASE_BRANCH_NAME
```

Once that's done, make sure to cherry-pick the fixes as well as the version bump from the previous
step into this branch:

```console
git cherry-pick -x <commit_id>
```

If anyone is available, let others peer-review the branch before continuing to ensure that all the
proper fixes have been merged into the release branch.

When you're finally happy with the quality of the release branch, you can move on and create the
tag. Double-check one last time to make sure everything is in order, then finally push the release
tag.

```console
git tag --sign --annotate "${RELEASE_NAME}" --message "Krustlet release ${RELEASE_NAME}"
git push upstream $RELEASE_NAME
```

It is usually more beneficial to the end-user if the release notes are hand-written by a human
being/marketing team/dog, so we'll go ahead and write up the release notes.

For patch releases, make note of the symptoms causing the original issue(s), who may be affected,
and how the patch(es) mitigate the issue(s).

it should look like this:

```markdown
## Krustlet vX.Y.Z

Krustlet vX.Y.Z is a patch release, fixing an issue where <insert disagnosis of issue here>.

This patch fixes this issue by <insert remedy here>.

Users are encouraged to upgrade for the best experience.

## Breaking changes

No known breaking changes were introduced this release.

## Installation

Download Krustlet vX.Y.Z:

- [checksums-vX.Y.Z.txt](https://krustlet.blob.core.windows.net/releases/checksums-vX.Y.Z.txt)
- [krustlet-vX.Y.Z-linux-amd64.tar.gz](https://krustlet.blob.core.windows.net/releases/krustlet-vX.Y.Z-linux-amd64.tar.gz)
- [krustlet-vX.Y.Z-macos-amd64.tar.gz](https://krustlet.blob.core.windows.net/releases/krustlet-vX.Y.Z-macos-amd64.tar.gz)
```

Double check the URLs are correct. Once finished, go into GitHub and edit the release notes for the
tagged release with the notes written here.

It is now worth getting other people to take a look at the release notes before the release is
published. It is always beneficial as it can be easy to miss something.

For pre-v1.0 releases, make sure to check the checkbox that says "This is a pre-release" to notify
users that this release is identified as non-production ready.

When you are ready to go, hit `publish`, and you're done.
