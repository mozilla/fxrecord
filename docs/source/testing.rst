Testing
=======

Tests are managed with cargo. To run tests:

.. code-block::

   cargo test

The integration tests support logging to aid in debugging, but by default cargo
runs tests in parallel. You can either run the test sequentually with:

.. code-block::

   cargo test -p integration-tests -- --test-threads 1

or run a specific test with:

.. code-block::

   cargo test -p integration-tests -- test_name

Cargo also captures output by default. To include the logs in the output of the
tests, you can run:

.. code-block::

   cargo test -- --nocapture

but it is recommended to either run tests sequentially or run a specific test,
as the log output from each test will be interwoven.
