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

.. note::

   There is currently an |issue27|_. Usually running the test in release mode
   without capturing standard output causes the test to pass, for example:

   .. code-block::

      cargo test --release -p integration_tests -- integration_tests::test_resume_session_ok --nocapture


.. |issue27| replace:: an intermittent failure in the integration test ``test_resume_session_ok``
.. _issue27: https://github.com/mozilla/fxrecord/issues/27
