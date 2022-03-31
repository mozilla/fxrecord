Requirements
============

Build Requirements
------------------

fxrecord is built with `cargo`_ and requires Rust 1.39+ for async/await support.

The only supported operating system for both fxrecorder and fxrunner is
Windows 10.


fxrecorder
----------

fxrecorder requires the following:

- an installation of `Python 3`_;
- a capture card compatible with ffmpeg;
- `ImageMagick 6.9 and ffmpeg 4.2+ <imagemagick_>`_

The only capture cards that have been tested and verified to work are:

- `AverMedia Live Gamer EXTREME 2 (Gc551) <gc551_>`_
- `Elgato Game Capture HD60 S <hd60s_>`_

See the :doc:`deployment` section for details on installation in a production setting.


.. _cargo: https://rustup.rs/
.. _Python 3: https//python.org/
.. _imagemagick: https://legacy.imagemagick.org/
.. _gc551: https://www.avermedia.com/us/product-detail/GC551
.. _hd60s: https://www.elgato.com/en/game-capture-hd60-s
