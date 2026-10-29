// Arnes COMPAT (Golden Master). Las sondas solo informan por console.error, que es el unico
// nivel que el worker registra (DELTA_10 D42). Cada linea cabe en el tope de 512 B.
"use strict";
var PROBE = {
  results: [],
  check: function (key, fn) {
    var v;
    try {
      v = fn();
      if (v === true) v = "1";
      else if (v === false) v = "0";
      else if (v === "" || v === null || v === undefined) v = "-";
      else v = String(v);
    } catch (e) {
      v = "E:" + ((e && e.name) || "error");
    }
    this.results.push(key + "=" + v);
  },
  emit: function (tag) {
    var line = "";
    for (var i = 0; i < this.results.length; i++) {
      var item = this.results[i];
      if (line.length + item.length + 1 > 380) {
        console.error("[probe:" + tag + "] " + line);
        line = "";
      }
      line += (line ? ";" : "") + item;
    }
    if (line) console.error("[probe:" + tag + "] " + line);
    this.results = [];
  },
  done: function (tag) {
    console.error("[probe:done] " + tag);
  }
};
