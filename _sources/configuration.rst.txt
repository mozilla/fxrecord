Configuration
=============

fxrunner
--------

:program:`fxrunner` is managed by a config file named :file:`fxrecord.toml`:

.. code-block:: toml

   [fxrunner]
   # The host and port fxrunner will listen on.
   host = "0.0.0.0:8888"

   # The directory to store sessions (downloaded builds of Firefox and profiles)
   # to persist through reboots.
   session_dir = "C:\\fxrunner\\sessions"

   # The size of the display.
   display_size = { x = 1366, y = 768 }


fxrecorder
----------

:program:`fxrecorder` is managed by a config file name :file:`fxrecord.toml`:

.. code-block:: toml

   [fxrecorder]
   # The host and port that fxrunner is listening on. Hostnames are supported.
   host = "127.0.0.1:8888"

   # The path to vendor/visualmetrics.py
   visual_metrics_path = "c:\\fxrecorder\\vendor\\visualmetrics.py"

   [fxrecorder.recording]
   # The resolution captured by the capture card.
   video_size = { x = 1920, y = 1080 }

   # The output size of the video. This should match `fxrunner.display_size`.
   output_size = { x = 1366, y = 768 }

   # The frame rate of the capture card.
   frame_rate = 60

   # The name of the capture card as detected by ffmpeg.
   device = "Game Capture HD60 S"

   # The size of the buffer for capturing video while encoding. At least 1GB is recommended.
   buffer_size = "1000M"

   # The minimum time a recording can take.
   minimum_recording_time_secs = 60


To determine the name of your capture card, you can run:

.. code-block::

   ffmpeg -hide_banner -list_devices -f dshow -i dummy

which will provide output like:

.. code-block::

   [dshow @ 000001a2656ad240] DirectShow video devices (some may be both video and audio devices)
   [dshow @ 000001a2656ad240]  "AVerMedia GC551 Video Capture"
   [dshow @ 000001a2656ad240]  "Game Capture HD60 S"

The quoted names are the values the configuration accepts.
