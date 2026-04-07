# State management

State management is one of the hard problems when designing UI applications.
In any not trivial application, state will diverge from rendering model very quickly.

Take for example simple dark/light theme switcher. Single component, affects rendering of
every element in the app.

More complex example would be if there is a toolbar but some other state influnces elements
in that toolbar.
