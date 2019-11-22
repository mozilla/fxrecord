# Test Files

This directory contains artifacts used for testing.

## firefox.zip

This is a sample build artifact containing a "Firefox executable" (an empty
file) used for mocking Taskcluster responses.

## profile.zip

This file is a sample profile containing some files present in Firefox
profiles. It is used to test profile transfer.

## profile_nested.zip

This is sample profile containing the same files as `profile.zip`, but nested
under a top-level `profile` directory. It is used to test profile transfer.

## test.zip

A test file used to verify that files and directories (even empty ones) are
created correctly.
