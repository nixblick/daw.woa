/* WOA – Wir on Air · Referenz-Verhalten der Hardware-Komponenten (vanilla, kein Framework).
   window.WOA: icon, mark, theme, meter, knob, fader, clock, wave, vu, bars, levels, num, tips, toast, init. */
(function () {
  'use strict';
  /* Phosphor Icons 2.1.1 (MIT, phosphoricons.com) – Regular, Transport in Fill. Pfade unverändert. */
  var P = {"play":"<path d=\"M240,128a15.74,15.74,0,0,1-7.6,13.51L88.32,229.65a16,16,0,0,1-16.2.3A15.86,15.86,0,0,1,64,216.13V39.87a15.86,15.86,0,0,1,8.12-13.82,16,16,0,0,1,16.2.3L232.4,114.49A15.74,15.74,0,0,1,240,128Z\"/>","pause":"<path d=\"M216,48V208a16,16,0,0,1-16,16H160a16,16,0,0,1-16-16V48a16,16,0,0,1,16-16h40A16,16,0,0,1,216,48ZM96,32H56A16,16,0,0,0,40,48V208a16,16,0,0,0,16,16H96a16,16,0,0,0,16-16V48A16,16,0,0,0,96,32Z\"/>","stop":"<path d=\"M216,56V200a16,16,0,0,1-16,16H56a16,16,0,0,1-16-16V56A16,16,0,0,1,56,40H200A16,16,0,0,1,216,56Z\"/>","rec":"<path d=\"M128,24A104,104,0,1,0,232,128,104.11,104.11,0,0,0,128,24Zm0,192a88,88,0,1,1,88-88A88.1,88.1,0,0,1,128,216Zm72-88a72,72,0,1,1-72-72A72.08,72.08,0,0,1,200,128Z\"/>","prev":"<path d=\"M199.81,34a16,16,0,0,0-16.24.43L64,109.23V40a8,8,0,0,0-16,0V216a8,8,0,0,0,16,0V146.77l119.57,74.78A15.95,15.95,0,0,0,208,208.12V47.88A15.86,15.86,0,0,0,199.81,34ZM192,208,64.16,128,192,48.07Z\"/>","next":"<path d=\"M200,32a8,8,0,0,0-8,8v69.23L72.43,34.45A15.95,15.95,0,0,0,48,47.88V208.12a16,16,0,0,0,24.43,13.43L192,146.77V216a8,8,0,0,0,16,0V40A8,8,0,0,0,200,32ZM64,207.93V48.05l127.84,80Z\"/>","loop":"<path d=\"M24,128A72.08,72.08,0,0,1,96,56H204.69L194.34,45.66a8,8,0,0,1,11.32-11.32l24,24a8,8,0,0,1,0,11.32l-24,24a8,8,0,0,1-11.32-11.32L204.69,72H96a56.06,56.06,0,0,0-56,56,8,8,0,0,1-16,0Zm200-8a8,8,0,0,0-8,8,56.06,56.06,0,0,1-56,56H51.31l10.35-10.34a8,8,0,0,0-11.32-11.32l-24,24a8,8,0,0,0,0,11.32l24,24a8,8,0,0,0,11.32-11.32L51.31,200H160a72.08,72.08,0,0,0,72-72A8,8,0,0,0,224,120Z\"/>","mic":"<path d=\"M128,176a48.05,48.05,0,0,0,48-48V64a48,48,0,0,0-96,0v64A48.05,48.05,0,0,0,128,176ZM96,64a32,32,0,0,1,64,0v64a32,32,0,0,1-64,0Zm40,143.6V240a8,8,0,0,1-16,0V207.6A80.11,80.11,0,0,1,48,128a8,8,0,0,1,16,0,64,64,0,0,0,128,0,8,8,0,0,1,16,0A80.11,80.11,0,0,1,136,207.6Z\"/>","phones":"<path d=\"M201.89,54.66A103.43,103.43,0,0,0,128.79,24H128A104,104,0,0,0,24,128v56a24,24,0,0,0,24,24H64a24,24,0,0,0,24-24V144a24,24,0,0,0-24-24H40.36A88,88,0,0,1,128,40h.67a87.71,87.71,0,0,1,87,80H192a24,24,0,0,0-24,24v40a24,24,0,0,0,24,24h16a24,24,0,0,0,24-24V128A103.41,103.41,0,0,0,201.89,54.66ZM64,136a8,8,0,0,1,8,8v40a8,8,0,0,1-8,8H48a8,8,0,0,1-8-8V136Zm152,48a8,8,0,0,1-8,8H192a8,8,0,0,1-8-8V144a8,8,0,0,1,8-8h24Z\"/>","antenna":"<path d=\"M128,88a40,40,0,1,0,40,40A40,40,0,0,0,128,88Zm0,64a24,24,0,1,1,24-24A24,24,0,0,1,128,152Zm73.71,7.14a80,80,0,0,1-14.08,22.2,8,8,0,0,1-11.92-10.67,63.95,63.95,0,0,0,0-85.33,8,8,0,1,1,11.92-10.67,80.08,80.08,0,0,1,14.08,84.47ZM69,103.09a64,64,0,0,0,11.26,67.58,8,8,0,0,1-11.92,10.67,79.93,79.93,0,0,1,0-106.67A8,8,0,1,1,80.29,85.34,63.77,63.77,0,0,0,69,103.09ZM248,128a119.58,119.58,0,0,1-34.29,84,8,8,0,1,1-11.42-11.2,103.9,103.9,0,0,0,0-145.56A8,8,0,1,1,213.71,44,119.58,119.58,0,0,1,248,128ZM53.71,200.78A8,8,0,1,1,42.29,212a119.87,119.87,0,0,1,0-168,8,8,0,1,1,11.42,11.2,103.9,103.9,0,0,0,0,145.56Z\"/>","sliders":"<path d=\"M136,120v96a8,8,0,0,1-16,0V120a8,8,0,0,1,16,0Zm64,72a8,8,0,0,0-8,8v16a8,8,0,0,0,16,0V200A8,8,0,0,0,200,192Zm24-32H208V40a8,8,0,0,0-16,0V160H176a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16ZM56,160a8,8,0,0,0-8,8v48a8,8,0,0,0,16,0V168A8,8,0,0,0,56,160Zm24-32H64V40a8,8,0,0,0-16,0v88H32a8,8,0,0,0,0,16H80a8,8,0,0,0,0-16Zm72-48H136V40a8,8,0,0,0-16,0V80H104a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Z\"/>","disc":"<path d=\"M128,24A104,104,0,1,0,232,128,104.11,104.11,0,0,0,128,24Zm0,192a88,88,0,1,1,88-88A88.1,88.1,0,0,1,128,216Zm0-144a56.06,56.06,0,0,0-56,56,8,8,0,0,1-16,0,72.08,72.08,0,0,1,72-72,8,8,0,0,1,0,16Zm72,56a72.08,72.08,0,0,1-72,72,8,8,0,0,1,0-16,56.06,56.06,0,0,0,56-56,8,8,0,0,1,16,0Zm-40,0a32,32,0,1,0-32,32A32,32,0,0,0,160,128Zm-48,0a16,16,0,1,1,16,16A16,16,0,0,1,112,128Z\"/>","heart":"<path d=\"M178,40c-20.65,0-38.73,8.88-50,23.89C116.73,48.88,98.65,40,78,40a62.07,62.07,0,0,0-62,62c0,70,103.79,126.66,108.21,129a8,8,0,0,0,7.58,0C136.21,228.66,240,172,240,102A62.07,62.07,0,0,0,178,40ZM128,214.8C109.74,204.16,32,155.69,32,102A46.06,46.06,0,0,1,78,56c19.45,0,35.78,10.36,42.6,27a8,8,0,0,0,14.8,0c6.82-16.67,23.15-27,42.6-27a46.06,46.06,0,0,1,46,46C224,155.61,146.24,204.15,128,214.8Z\"/>","plus":"<path d=\"M224,128a8,8,0,0,1-8,8H136v80a8,8,0,0,1-16,0V136H40a8,8,0,0,1,0-16h80V40a8,8,0,0,1,16,0v80h80A8,8,0,0,1,224,128Z\"/>","search":"<path d=\"M229.66,218.34l-50.07-50.06a88.11,88.11,0,1,0-11.31,11.31l50.06,50.07a8,8,0,0,0,11.32-11.32ZM40,112a72,72,0,1,1,72,72A72.08,72.08,0,0,1,40,112Z\"/>","x":"<path d=\"M205.66,194.34a8,8,0,0,1-11.32,11.32L128,139.31,61.66,205.66a8,8,0,0,1-11.32-11.32L116.69,128,50.34,61.66A8,8,0,0,1,61.66,50.34L128,116.69l66.34-66.35a8,8,0,0,1,11.32,11.32L139.31,128Z\"/>","up":"<path d=\"M213.66,165.66a8,8,0,0,1-11.32,0L128,91.31,53.66,165.66a8,8,0,0,1-11.32-11.32l80-80a8,8,0,0,1,11.32,0l80,80A8,8,0,0,1,213.66,165.66Z\"/>","down":"<path d=\"M213.66,101.66l-80,80a8,8,0,0,1-11.32,0l-80-80A8,8,0,0,1,53.66,90.34L128,164.69l74.34-74.35a8,8,0,0,1,11.32,11.32Z\"/>","cut":"<path d=\"M157.73,113.13A8,8,0,0,1,159.82,102L227.48,55.7a8,8,0,0,1,9,13.21l-67.67,46.3a7.92,7.92,0,0,1-4.51,1.4A8,8,0,0,1,157.73,113.13Zm80.87,85.09a8,8,0,0,1-11.12,2.08L136,137.7,93.49,166.78a36,36,0,1,1-9-13.19L121.83,128,84.44,102.41a35.86,35.86,0,1,1,9-13.19l143,97.87A8,8,0,0,1,238.6,198.22ZM80,180a20,20,0,1,0-5.86,14.14A19.85,19.85,0,0,0,80,180ZM74.14,90.13a20,20,0,1,0-28.28,0A19.85,19.85,0,0,0,74.14,90.13Z\"/>","note":"<path d=\"M210.3,56.34l-80-24A8,8,0,0,0,120,40V148.26A48,48,0,1,0,136,184V98.75l69.7,20.91A8,8,0,0,0,216,112V64A8,8,0,0,0,210.3,56.34ZM88,216a32,32,0,1,1,32-32A32,32,0,0,1,88,216ZM200,101.25l-64-19.2V50.75L200,70Z\"/>","feedback":"<path d=\"M216,48H40A16,16,0,0,0,24,64V224a15.85,15.85,0,0,0,9.24,14.5A16.13,16.13,0,0,0,40,240a15.89,15.89,0,0,0,10.25-3.78l.09-.07L83,208H216a16,16,0,0,0,16-16V64A16,16,0,0,0,216,48ZM40,224h0ZM216,192H80a8,8,0,0,0-5.23,1.95L40,224V64H216ZM88,112a8,8,0,0,1,8-8h64a8,8,0,0,1,0,16H96A8,8,0,0,1,88,112Zm0,32a8,8,0,0,1,8-8h64a8,8,0,1,1,0,16H96A8,8,0,0,1,88,144Z\"/>","fade":"<path d=\"M237.66,178.34a8,8,0,0,1,0,11.32l-24,24a8,8,0,0,1-11.32-11.32L212.69,192H200.94a72.12,72.12,0,0,1-58.59-30.15l-41.72-58.4A56.1,56.1,0,0,0,55.06,80H32a8,8,0,0,1,0-16H55.06a72.12,72.12,0,0,1,58.59,30.15l41.72,58.4A56.1,56.1,0,0,0,200.94,176h11.75l-10.35-10.34a8,8,0,0,1,11.32-11.32ZM143,107a8,8,0,0,0,11.16-1.86l1.2-1.67A56.1,56.1,0,0,1,200.94,80h11.75L202.34,90.34a8,8,0,0,0,11.32,11.32l24-24a8,8,0,0,0,0-11.32l-24-24a8,8,0,0,0-11.32,11.32L212.69,64H200.94a72.12,72.12,0,0,0-58.59,30.15l-1.2,1.67A8,8,0,0,0,143,107Zm-30,42a8,8,0,0,0-11.16,1.86l-1.2,1.67A56.1,56.1,0,0,1,55.06,176H32a8,8,0,0,0,0,16H55.06a72.12,72.12,0,0,0,58.59-30.15l1.2-1.67A8,8,0,0,0,113,149Z\"/>","wifi":"<path d=\"M140,204a12,12,0,1,1-12-12A12,12,0,0,1,140,204ZM237.08,87A172,172,0,0,0,18.92,87,8,8,0,0,0,29.08,99.37a156,156,0,0,1,197.84,0A8,8,0,0,0,237.08,87ZM205,122.77a124,124,0,0,0-153.94,0A8,8,0,0,0,61,135.31a108,108,0,0,1,134.06,0,8,8,0,0,0,11.24-1.3A8,8,0,0,0,205,122.77Zm-32.26,35.76a76.05,76.05,0,0,0-89.42,0,8,8,0,0,0,9.42,12.94,60,60,0,0,1,70.58,0,8,8,0,1,0,9.42-12.94Z\"/>","bolt":"<path d=\"M215.79,118.17a8,8,0,0,0-5-5.66L153.18,90.9l14.66-73.33a8,8,0,0,0-13.69-7l-112,120a8,8,0,0,0,3,13l57.63,21.61L88.16,238.43a8,8,0,0,0,13.69,7l112-120A8,8,0,0,0,215.79,118.17ZM109.37,214l10.47-52.38a8,8,0,0,0-5-9.06L62,132.71l84.62-90.66L136.16,94.43a8,8,0,0,0,5,9.06l52.8,19.8Z\"/>","trash":"<path d=\"M216,48H176V40a24,24,0,0,0-24-24H104A24,24,0,0,0,80,40v8H40a8,8,0,0,0,0,16h8V208a16,16,0,0,0,16,16H192a16,16,0,0,0,16-16V64h8a8,8,0,0,0,0-16ZM96,40a8,8,0,0,1,8-8h48a8,8,0,0,1,8,8v8H96Zm96,168H64V64H192ZM112,104v64a8,8,0,0,1-16,0V104a8,8,0,0,1,16,0Zm48,0v64a8,8,0,0,1-16,0V104a8,8,0,0,1,16,0Z\"/>","power":"<path d=\"M120,128V48a8,8,0,0,1,16,0v80a8,8,0,0,1-16,0Zm60.37-78.7a8,8,0,0,0-8.74,13.4C194.74,77.77,208,101.57,208,128a80,80,0,0,1-160,0c0-26.43,13.26-50.23,36.37-65.3a8,8,0,0,0-8.74-13.4C47.9,67.38,32,96.06,32,128a96,96,0,0,0,192,0C224,96.06,208.1,67.38,180.37,49.3Z\"/>","grip":"<path d=\"M104,60A12,12,0,1,1,92,48,12,12,0,0,1,104,60Zm60,12a12,12,0,1,0-12-12A12,12,0,0,0,164,72ZM92,116a12,12,0,1,0,12,12A12,12,0,0,0,92,116Zm72,0a12,12,0,1,0,12,12A12,12,0,0,0,164,116ZM92,184a12,12,0,1,0,12,12A12,12,0,0,0,92,184Zm72,0a12,12,0,1,0,12,12A12,12,0,0,0,164,184Z\"/>","window":"<path d=\"M216,40H40A16,16,0,0,0,24,56V200a16,16,0,0,0,16,16H216a16,16,0,0,0,16-16V56A16,16,0,0,0,216,40Zm0,160H40V56H216V200ZM80,84A12,12,0,1,1,68,72,12,12,0,0,1,80,84Zm40,0a12,12,0,1,1-12-12A12,12,0,0,1,120,84Z\"/>","plug":"<path d=\"M237.66,66.34a8,8,0,0,0-11.32,0L192,100.69,155.31,64l34.35-34.34a8,8,0,1,0-11.32-11.32L144,52.69,117.66,26.34a8,8,0,0,0-11.32,11.32L112.69,44l-53,53a40,40,0,0,0,0,56.57l15.71,15.71L26.34,218.34a8,8,0,0,0,11.32,11.32l49.09-49.09,15.71,15.71a40,40,0,0,0,56.57,0l53-53,6.34,6.35a8,8,0,0,0,11.32-11.32L203.31,112l34.35-34.34A8,8,0,0,0,237.66,66.34ZM147.72,185a24,24,0,0,1-33.95,0L71,142.23a24,24,0,0,1,0-33.95l53-53L200.69,132Z\"/>","warning":"<path d=\"M236.8,188.09,149.35,36.22h0a24.76,24.76,0,0,0-42.7,0L19.2,188.09a23.51,23.51,0,0,0,0,23.72A24.35,24.35,0,0,0,40.55,224h174.9a24.35,24.35,0,0,0,21.33-12.19A23.51,23.51,0,0,0,236.8,188.09ZM222.93,203.8a8.5,8.5,0,0,1-7.48,4.2H40.55a8.5,8.5,0,0,1-7.48-4.2,7.59,7.59,0,0,1,0-7.72L120.52,44.21a8.75,8.75,0,0,1,15,0l87.45,151.87A7.59,7.59,0,0,1,222.93,203.8ZM120,144V104a8,8,0,0,1,16,0v40a8,8,0,0,1-16,0Zm20,36a12,12,0,1,1-12-12A12,12,0,0,1,140,180Z\"/>","reload":"<path d=\"M240,56v48a8,8,0,0,1-8,8H184a8,8,0,0,1,0-16H211.4L184.81,71.64l-.25-.24a80,80,0,1,0-1.67,114.78,8,8,0,0,1,11,11.63A95.44,95.44,0,0,1,128,224h-1.32A96,96,0,1,1,195.75,60L224,85.8V56a8,8,0,1,1,16,0Z\"/>","copy":"<path d=\"M216,32H88a8,8,0,0,0-8,8V80H40a8,8,0,0,0-8,8V216a8,8,0,0,0,8,8H168a8,8,0,0,0,8-8V176h40a8,8,0,0,0,8-8V40A8,8,0,0,0,216,32ZM160,208H48V96H160Zm48-48H176V88a8,8,0,0,0-8-8H96V48H208Z\"/>","reverse":"<path d=\"M224,48V152a16,16,0,0,1-16,16H99.31l10.35,10.34a8,8,0,0,1-11.32,11.32l-24-24a8,8,0,0,1,0-11.32l24-24a8,8,0,0,1,11.32,11.32L99.31,152H208V48H96v8a8,8,0,0,1-16,0V48A16,16,0,0,1,96,32H208A16,16,0,0,1,224,48ZM168,192a8,8,0,0,0-8,8v8H48V104H156.69l-10.35,10.34a8,8,0,0,0,11.32,11.32l24-24a8,8,0,0,0,0-11.32l-24-24a8,8,0,0,0-11.32,11.32L156.69,88H48a16,16,0,0,0-16,16V208a16,16,0,0,0,16,16H160a16,16,0,0,0,16-16v-8A8,8,0,0,0,168,192Z\"/>","undo":"<path d=\"M224,128a96,96,0,0,1-94.71,96H128A95.38,95.38,0,0,1,62.1,197.8a8,8,0,0,1,11-11.63A80,80,0,1,0,71.43,71.39a3.07,3.07,0,0,1-.26.25L44.59,96H72a8,8,0,0,1,0,16H24a8,8,0,0,1-8-8V56a8,8,0,0,1,16,0V85.8L60.25,60A96,96,0,0,1,224,128Z\"/>","mute":"<path d=\"M53.92,34.62A8,8,0,1,0,42.08,45.38L73.55,80H32A16,16,0,0,0,16,96v64a16,16,0,0,0,16,16H77.25l69.84,54.31A8,8,0,0,0,160,224V175.09l42.08,46.29a8,8,0,1,0,11.84-10.76ZM32,96H72v64H32ZM144,207.64,88,164.09V95.89l56,61.6Zm42-63.77a24,24,0,0,0,0-31.72,8,8,0,1,1,12-10.57,40,40,0,0,1,0,52.88,8,8,0,0,1-12-10.59Zm-80.16-76a8,8,0,0,1,1.4-11.23l39.85-31A8,8,0,0,1,160,32v74.83a8,8,0,0,1-16,0V48.36l-26.94,21A8,8,0,0,1,105.84,67.91ZM248,128a79.9,79.9,0,0,1-20.37,53.34,8,8,0,0,1-11.92-10.67,64,64,0,0,0,0-85.33,8,8,0,1,1,11.92-10.67A79.83,79.83,0,0,1,248,128Z\"/>","speaker":"<path d=\"M155.51,24.81a8,8,0,0,0-8.42.88L77.25,80H32A16,16,0,0,0,16,96v64a16,16,0,0,0,16,16H77.25l69.84,54.31A8,8,0,0,0,160,224V32A8,8,0,0,0,155.51,24.81ZM32,96H72v64H32ZM144,207.64,88,164.09V91.91l56-43.55Zm54-106.08a40,40,0,0,1,0,52.88,8,8,0,0,1-12-10.58,24,24,0,0,0,0-31.72,8,8,0,0,1,12-10.58ZM248,128a79.9,79.9,0,0,1-20.37,53.34,8,8,0,0,1-11.92-10.67,64,64,0,0,0,0-85.33,8,8,0,1,1,11.92-10.67A79.83,79.83,0,0,1,248,128Z\"/>","wave":"<path d=\"M56,96v64a8,8,0,0,1-16,0V96a8,8,0,0,1,16,0ZM88,24a8,8,0,0,0-8,8V224a8,8,0,0,0,16,0V32A8,8,0,0,0,88,24Zm40,32a8,8,0,0,0-8,8V192a8,8,0,0,0,16,0V64A8,8,0,0,0,128,56Zm40,32a8,8,0,0,0-8,8v64a8,8,0,0,0,16,0V96A8,8,0,0,0,168,88Zm40-16a8,8,0,0,0-8,8v96a8,8,0,0,0,16,0V80A8,8,0,0,0,208,72Z\"/>","piano":"<path d=\"M208,32H48A16,16,0,0,0,32,48V208a16,16,0,0,0,16,16H208a16,16,0,0,0,16-16V48A16,16,0,0,0,208,32ZM80,48h24v88H80Zm32,104a8,8,0,0,0,8-8V48h16v96a8,8,0,0,0,8,8h8v56H104V152Zm40-16V48h24v88ZM48,48H64v96a8,8,0,0,0,8,8H88v56H48ZM208,208H168V152h16a8,8,0,0,0,8-8V48h16V208Z\"/>","magnet":"<path d=\"M207,50.25A87.46,87.46,0,0,0,144.6,24h-.33A87.48,87.48,0,0,0,82,49.81L20.61,112a16,16,0,0,0,.06,22.56l28.66,28.66a15.92,15.92,0,0,0,11.32,4.69h.09a16,16,0,0,0,11.36-4.82L133,100.69a16.08,16.08,0,0,1,22.41-.21,15.6,15.6,0,0,1,4.73,11.19,16.89,16.89,0,0,1-4.85,12L93,183.88a16,16,0,0,0-.17,22.79l28.66,28.66a16.06,16.06,0,0,0,22.52.12L205.81,175C240.26,140.5,240.79,84.56,207,50.25ZM60.65,151.89,32,123.24,55.8,99.12l28.52,28.52ZM132.79,224l-28.68-28.65,24.38-23.57L157,200.32Zm61.76-60.44-26.11,25.54L140,160.68l26.44-25.57.1-.09a33,33,0,0,0,9.57-23.5A31.44,31.44,0,0,0,166.47,89a32.2,32.2,0,0,0-44.9.5L95.49,116.18,67,87.74,93.35,61.09A71.51,71.51,0,0,1,144.27,40h.27a71.55,71.55,0,0,1,51.05,21.48C223.25,89.55,222.75,135.38,194.55,163.58Z\"/>","rename":"<path d=\"M227.31,73.37,182.63,28.68a16,16,0,0,0-22.63,0L36.69,152A15.86,15.86,0,0,0,32,163.31V208a16,16,0,0,0,16,16H92.69A15.86,15.86,0,0,0,104,219.31L227.31,96a16,16,0,0,0,0-22.63ZM92.69,208H48V163.31l88-88L180.69,120ZM192,108.68,147.31,64l24-24L216,84.68Z\"/>","palette":"<path d=\"M200.77,53.89A103.27,103.27,0,0,0,128,24h-1.07A104,104,0,0,0,24,128c0,43,26.58,79.06,69.36,94.17A32,32,0,0,0,136,192a16,16,0,0,1,16-16h46.21a31.81,31.81,0,0,0,31.2-24.88,104.43,104.43,0,0,0,2.59-24A103.28,103.28,0,0,0,200.77,53.89Zm13,93.71A15.89,15.89,0,0,1,198.21,160H152a32,32,0,0,0-32,32,16,16,0,0,1-21.31,15.07C62.49,194.3,40,164,40,128a88,88,0,0,1,87.09-88h.9a88.35,88.35,0,0,1,88,87.25A88.86,88.86,0,0,1,213.81,147.6ZM140,76a12,12,0,1,1-12-12A12,12,0,0,1,140,76ZM96,100A12,12,0,1,1,84,88,12,12,0,0,1,96,100Zm0,56a12,12,0,1,1-12-12A12,12,0,0,1,96,156Zm88-56a12,12,0,1,1-12-12A12,12,0,0,1,184,100Z\"/>","right":"<path d=\"M181.66,133.66l-80,80a8,8,0,0,1-11.32-11.32L164.69,128,90.34,53.66a8,8,0,0,1,11.32-11.32l80,80A8,8,0,0,1,181.66,133.66Z\"/>","check":"<path d=\"M229.66,77.66l-128,128a8,8,0,0,1-11.32,0l-56-56a8,8,0,0,1,11.32-11.32L96,188.69,218.34,66.34a8,8,0,0,1,11.32,11.32Z\"/>","output":"<path d=\"M221.66,133.66l-72,72a8,8,0,0,1-11.32-11.32L196.69,136H40a8,8,0,0,1,0-16H196.69L138.34,61.66a8,8,0,0,1,11.32-11.32l72,72A8,8,0,0,1,221.66,133.66Z\"/>","send":"<path d=\"M229.66,157.66l-48,48a8,8,0,0,1-11.32-11.32L204.69,160H128A104.11,104.11,0,0,1,24,56a8,8,0,0,1,16,0,88.1,88.1,0,0,0,88,88h76.69l-34.35-34.34a8,8,0,0,1,11.32-11.32l48,48A8,8,0,0,1,229.66,157.66Z\"/>","fadein":"<path d=\"M240,56v64a8,8,0,0,1-16,0V75.31l-82.34,82.35a8,8,0,0,1-11.32,0L96,123.31,29.66,189.66a8,8,0,0,1-11.32-11.32l72-72a8,8,0,0,1,11.32,0L136,140.69,212.69,64H168a8,8,0,0,1,0-16h64A8,8,0,0,1,240,56Z\"/>","fadeout":"<path d=\"M240,128v64a8,8,0,0,1-8,8H168a8,8,0,0,1,0-16h44.69L136,107.31l-34.34,34.35a8,8,0,0,1-11.32,0l-72-72A8,8,0,0,1,29.66,58.34L96,124.69l34.34-34.35a8,8,0,0,1,11.32,0L224,172.69V128a8,8,0,0,1,16,0Z\"/>","normalize":"<path d=\"M224,128a8,8,0,0,1-8,8H40a8,8,0,0,1,0-16H216A8,8,0,0,1,224,128ZM101.66,53.66,120,35.31V96a8,8,0,0,0,16,0V35.31l18.34,18.35a8,8,0,0,0,11.32-11.32l-32-32a8,8,0,0,0-11.32,0l-32,32a8,8,0,0,0,11.32,11.32Zm52.68,148.68L136,220.69V160a8,8,0,0,0-16,0v60.69l-18.34-18.35a8,8,0,0,0-11.32,11.32l32,32a8,8,0,0,0,11.32,0l32-32a8,8,0,0,0-11.32-11.32Z\"/>","tempo":"<path d=\"M187.14,114.84l26.78-29.46a8,8,0,0,0-11.84-10.76l-20.55,22.6-17.2-54.07A15.94,15.94,0,0,0,149.08,32H106.91A15.94,15.94,0,0,0,91.66,43.15l-50.91,160A16,16,0,0,0,56,224H200a16,16,0,0,0,15.25-20.85ZM184.72,160H146.08l28.62-31.48ZM106.91,48h42.17l20,62.9L124.46,160H71.27ZM56,208l10.18-32H189.81L200,208Z\"/>","pitch":"<path d=\"M212.92,17.69a8,8,0,0,0-6.86-1.45l-128,32A8,8,0,0,0,72,56V166.08A36,36,0,1,0,88,196V110.25l112-28v51.83A36,36,0,1,0,216,164V24A8,8,0,0,0,212.92,17.69ZM52,216a20,20,0,1,1,20-20A20,20,0,0,1,52,216ZM88,93.75V62.25l112-28v31.5ZM180,184a20,20,0,1,1,20-20A20,20,0,0,1,180,184Z\"/>","quantize":"<path d=\"M200,40H56A16,16,0,0,0,40,56V200a16,16,0,0,0,16,16H200a16,16,0,0,0,16-16V56A16,16,0,0,0,200,40Zm0,80H136V56h64ZM120,56v64H56V56ZM56,136h64v64H56Zm144,64H136V136h64v64Z\"/>","more":"<path d=\"M140,128a12,12,0,1,1-12-12A12,12,0,0,1,140,128Zm56-12a12,12,0,1,0,12,12A12,12,0,0,0,196,116ZM60,116a12,12,0,1,0,12,12A12,12,0,0,0,60,116Z\"/>","open":"<path d=\"M224,104a8,8,0,0,1-16,0V59.32l-66.33,66.34a8,8,0,0,1-11.32-11.32L196.68,48H152a8,8,0,0,1,0-16h64a8,8,0,0,1,8,8Zm-40,24a8,8,0,0,0-8,8v72H48V80h72a8,8,0,0,0,0-16H48A16,16,0,0,0,32,80V208a16,16,0,0,0,16,16H176a16,16,0,0,0,16-16V136A8,8,0,0,0,184,128Z\"/>","stack":"<path d=\"M230.91,172A8,8,0,0,1,228,182.91l-96,56a8,8,0,0,1-8.06,0l-96-56A8,8,0,0,1,36,169.09l92,53.65,92-53.65A8,8,0,0,1,230.91,172ZM220,121.09l-92,53.65L36,121.09A8,8,0,0,0,28,134.91l96,56a8,8,0,0,0,8.06,0l96-56A8,8,0,1,0,220,121.09ZM24,80a8,8,0,0,1,4-6.91l96-56a8,8,0,0,1,8.06,0l96,56a8,8,0,0,1,0,13.82l-96,56a8,8,0,0,1-8.06,0l-96-56A8,8,0,0,1,24,80Zm23.88,0L128,126.74,208.12,80,128,33.26Z\"/>","folder":"<path d=\"M245,110.64A16,16,0,0,0,232,104H216V88a16,16,0,0,0-16-16H130.67L102.94,51.2a16.14,16.14,0,0,0-9.6-3.2H40A16,16,0,0,0,24,64V208h0a8,8,0,0,0,8,8H211.1a8,8,0,0,0,7.59-5.47l28.49-85.47A16.05,16.05,0,0,0,245,110.64ZM93.34,64,123.2,86.4A8,8,0,0,0,128,88h72v16H69.77a16,16,0,0,0-15.18,10.94L40,158.7V64Zm112,136H43.1l26.67-80H232Z\"/>","info":"<path d=\"M128,24A104,104,0,1,0,232,128,104.11,104.11,0,0,0,128,24Zm0,192a88,88,0,1,1,88-88A88.1,88.1,0,0,1,128,216Zm16-40a8,8,0,0,1-8,8,16,16,0,0,1-16-16V128a8,8,0,0,1,0-16,16,16,0,0,1,16,16v40A8,8,0,0,1,144,176ZM112,84a12,12,0,1,1,12,12A12,12,0,0,1,112,84Z\"/>","eq":"<path d=\"M80,96a8,8,0,0,1-8,8H24a8,8,0,0,1,0-16H72A8,8,0,0,1,80,96Zm-8,24H24a8,8,0,0,0,0,16H72a8,8,0,0,0,0-16Zm0,32H24a8,8,0,0,0,0,16H72a8,8,0,0,0,0-16Zm0,32H24a8,8,0,0,0,0,16H72a8,8,0,0,0,0-16Zm80-64H104a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Zm0,32H104a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Zm0,32H104a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Zm80-96H184a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16ZM184,72h48a8,8,0,0,0,0-16H184a8,8,0,0,0,0,16Zm48,48H184a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Zm0,32H184a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Zm0,32H184a8,8,0,0,0,0,16h48a8,8,0,0,0,0-16Z\"/>","merge":"<path d=\"M192,40v64a8,8,0,0,1-2.34,5.66L136,163.31v49.38l18.34-18.35a8,8,0,0,1,11.32,11.32l-32,32a8,8,0,0,1-11.32,0l-32-32a8,8,0,0,1,11.32-11.32L120,212.69V163.31L66.34,109.66A8,8,0,0,1,64,104V40a8,8,0,0,1,16,0v60.69l48,48,48-48V40a8,8,0,0,1,16,0Z\"/>","cursor":"<path d=\"M88,24V16a8,8,0,0,1,16,0v8a8,8,0,0,1-16,0ZM16,104h8a8,8,0,0,0,0-16H16a8,8,0,0,0,0,16ZM124.42,39.16a8,8,0,0,0,10.74-3.58l8-16a8,8,0,0,0-14.31-7.16l-8,16A8,8,0,0,0,124.42,39.16Zm-96,81.69-16,8a8,8,0,0,0,7.16,14.31l16-8a8,8,0,1,0-7.16-14.31ZM219.31,184a16,16,0,0,1,0,22.63l-12.68,12.68a16,16,0,0,1-22.63,0L132.7,168,115,214.09c0,.1-.08.21-.13.32a15.83,15.83,0,0,1-14.6,9.59l-.79,0a15.83,15.83,0,0,1-14.41-11L32.8,52.92A16,16,0,0,1,52.92,32.8L213,85.07a16,16,0,0,1,1.41,29.8l-.32.13L168,132.69ZM208,195.31,156.69,144h0a16,16,0,0,1,4.93-26l.32-.14,45.95-17.64L48,48l52.2,159.86,17.65-46c0-.11.08-.22.13-.33a16,16,0,0,1,11.69-9.34,16.72,16.72,0,0,1,3-.28,16,16,0,0,1,11.3,4.69L195.31,208Z\"/>"};
  function icon(name, cls) {
    return '<svg class="woa-i ' + (cls || '') + '" viewBox="0 0 256 256" aria-hidden="true">' + (P[name] || '') + '</svg>';
  }
  /* Bildmarke: fünf LED-Säulen in W-Form + ON-AIR-Lampe. Säulen in Modusfarbe, Lampe immer on-air. */
  function mark(size) {
    var H = [144, 72, 112, 72, 144], s = '';
    for (var i = 0; i < 5; i++) {
      var x = 44 + i * 36, h = H[i], n = Math.floor((h + 6) / 20), y = 204;
      for (var k = 0; k < n; k++) {
        y -= 14;
        s += '<rect x="' + x + '" y="' + y + '" width="24" height="14" rx="3" style="fill:var(' + (k === n - 1 ? '--accent-strong' : '--accent') + ')"/>';
        y -= 6;
      }
    }
    s += '<circle cx="128" cy="54" r="22" style="fill:var(--on-air)" opacity=".22"/><circle cx="128" cy="54" r="15" style="fill:var(--on-air)"/>';
    return '<svg viewBox="36 28 184 184" width="' + (size || 26) + '" height="' + (size || 26) + '" role="img" aria-label="WOA">' + s + '</svg>';
  }
  /* Modus-Thema auf ein Element: folgt hell/dunkel der Seite. */
  function theme(el, mode) {
    function apply() {
      var t = document.documentElement.getAttribute('data-theme') || '';
      el.setAttribute('data-theme', /-hell$/.test(t) ? mode + '-hell' : mode);
    }
    apply();
    new MutationObserver(apply).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
  }
  var reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  var meters = [];
  function zone(f) { return f > 0.95 ? 'r' : f > 0.8 ? 'y' : 'g'; } /* -60..0 dB: >-3 rot, >-12 gelb */
  function meter(el, o) {
    o = o || {};
    var ch = +(el.dataset.ch || o.ch || 2), n = +(el.dataset.segs || o.segs || 30), html = '';
    for (var c = 0; c < ch; c++) {
      html += '<div class="woa-meter__ch">';
      for (var i = 0; i < n; i++) html += '<div class="woa-meter__seg" data-z="' + zone((i + 1) / n) + '"></div>';
      html += '</div>';
    }
    el.innerHTML = html;
    var m = { el: el, n: n, ch: [].slice.call(el.children), lv: [], tgt: +(el.dataset.level || 0.7), peak: [], live: el.dataset.static === undefined };
    m.ch.forEach(function (_, i) { m.lv[i] = m.tgt; m.peak[i] = m.tgt; });
    meters.push(m); paint(m); return m;
  }
  function paint(m) {
    m.ch.forEach(function (c, i) {
      var lit = Math.round(m.lv[i] * m.n), pk = Math.min(m.n - 1, Math.round(m.peak[i] * m.n) - 1);
      for (var k = 0; k < m.n; k++) c.children[k].classList.toggle('on', k < lit || k === pk);
    });
  }
  var t0 = 0;
  function tick(t) {
    if (t - t0 > 55) {
      t0 = t;
      meters.forEach(function (m) {
        if (!m.live) return;
        m.ch.forEach(function (_, i) {
          var target = m.tgt * (0.72 + Math.random() * 0.34);
          m.lv[i] += (target - m.lv[i]) * (target > m.lv[i] ? 0.7 : 0.25);
          m.lv[i] = Math.max(0, Math.min(1, m.lv[i]));
          m.peak[i] = m.lv[i] > m.peak[i] ? m.lv[i] : m.peak[i] - 0.006;
        });
        paint(m);
      });
    }
    requestAnimationFrame(tick);
  }
  if (!reduce) requestAnimationFrame(tick);
  function drag(el, target, horizontal, fmt, out) {
    var v = parseFloat(getComputedStyle(el).getPropertyValue('--v')) || 0;
    function set(nv) { v = Math.max(0, Math.min(1, nv)); el.style.setProperty('--v', v.toFixed(3)); if (out) out.textContent = fmt(v); }
    set(v);
    target.addEventListener('pointerdown', function (e) {
      target.setPointerCapture(e.pointerId);
      var sx = e.clientX, sy = e.clientY, sv = v, len = horizontal ? target.clientWidth : (target.clientHeight || 150);
      function mv(ev) { set(sv + (horizontal ? (ev.clientX - sx) : (sy - ev.clientY)) / len); }
      function up() { target.removeEventListener('pointermove', mv); target.removeEventListener('pointerup', up); }
      target.addEventListener('pointermove', mv); target.addEventListener('pointerup', up);
    });
    target.addEventListener('dblclick', function () { set(+(el.dataset.default || 0.5)); });
    target.tabIndex = 0;
    target.addEventListener('keydown', function (e) {
      var d = { ArrowUp: .02, ArrowRight: .02, ArrowDown: -.02, ArrowLeft: -.02 }[e.key];
      if (d) { e.preventDefault(); set(v + d * (e.shiftKey ? 5 : 1)); }
    });
  }
  var FMT = {
    db: function (v) { var d = v <= 0 ? -Infinity : 20 * Math.log10(Math.pow(v / 0.75, 2.5)); return d === -Infinity ? '−∞' : (d > 0 ? '+' : '') + d.toFixed(1); },
    pan: function (v) { var p = Math.round((v - .5) * 200); return p === 0 ? 'C' : (p < 0 ? 'L' + -p : 'R' + p); },
    eq: function (v) { var d = (v - .5) * 30; return (d > 0 ? '+' : '') + d.toFixed(1); },
    pct: function (v) { return Math.round(v * 100) + '%'; }
  };
  function knob(el) { drag(el, el.querySelector('.woa-knob__dial'), false, FMT[el.dataset.fmt || 'pct'], el.querySelector('.woa-knob__val')); }
  function fader(el) { drag(el, el.querySelector('.woa-fader__track'), el.classList.contains('woa-fader--h'), FMT[el.dataset.fmt || 'db'], document.getElementById(el.dataset.out || '')); }
  function clock(el) {
    var html = '', R = 0.39, Rh = 0.465;
    for (var i = 0; i < 60; i++) {
      var a = i / 60 * 2 * Math.PI;
      html += '<i class="woa-clock__dot" style="transform:translate(' + (Math.sin(a) * R * 100).toFixed(2) + 'cqw,' + (-Math.cos(a) * R * 100).toFixed(2) + 'cqw)"></i>';
    }
    for (var h = 0; h < 12; h++) {
      var b = h / 12 * 2 * Math.PI;
      html += '<i class="woa-clock__dot h" style="transform:translate(' + (Math.sin(b) * Rh * 100).toFixed(2) + 'cqw,' + (-Math.cos(b) * Rh * 100).toFixed(2) + 'cqw)"></i>';
    }
    html += '<div class="woa-clock__mid"><span class="woa-seg woa-clock__time" data-ghost="88:88"><span></span></span><span class="woa-seg woa-clock__sec" data-ghost="88"><span></span></span></div>';
    el.style.containerType = 'inline-size';
    el.innerHTML = html;
    var dots = el.querySelectorAll('.woa-clock__dot:not(.h)'), hd = el.querySelectorAll('.woa-clock__dot.h');
    var tm = el.querySelector('.woa-clock__time span'), sc = el.querySelector('.woa-clock__sec span');
    var fixed = el.dataset.time;
    function up() {
      var d = new Date(), H = d.getHours(), M = d.getMinutes(), S = d.getSeconds();
      if (fixed) { var p = fixed.split(':'); H = +p[0]; M = +p[1]; S = +p[2]; }
      for (var i = 0; i < 60; i++) dots[i].classList.toggle('on', i <= S);
      for (var j = 0; j < 12; j++) hd[j].classList.toggle('on', true);
      tm.textContent = ('0' + H).slice(-2) + ':' + ('0' + M).slice(-2);
      sc.textContent = ('0' + S).slice(-2);
    }
    up(); if (!fixed) setInterval(up, 1000);
  }
  /* Analoges VU-Meter: Zeiger −20…+3 VU, Skala per SVG. */
  var vus = [];
  function vu(el) {
    var ticks = [[-20, '20'], [-10, '10'], [-7, '7'], [-5, '5'], [-3, '3'], [-2, ''], [-1, ''], [0, '0'], [1, ''], [2, ''], [3, '+3']], s = '';
    function ang(db) { var lin = Math.pow(10, db / 20), f = (lin - 0.1) / (1.413 - 0.1); return -48 + f * 96; }
    function pt(a, r) { var t = (a - 90) * Math.PI / 180; return [100 + r * Math.cos(t), 128 + r * Math.sin(t)]; }
    function arc(a0, a1, r) { var p0 = pt(a0, r), p1 = pt(a1, r); return 'M' + p0[0].toFixed(1) + ' ' + p0[1].toFixed(1) + 'A' + r + ' ' + r + ' 0 0 1 ' + p1[0].toFixed(1) + ' ' + p1[1].toFixed(1); }
    s += '<path d="' + arc(-48, ang(0), 92) + '" fill="none" style="stroke:var(--vu-ink)" stroke-width="1.5"/>';
    s += '<path d="' + arc(ang(0), 48, 92) + '" fill="none" style="stroke:var(--on-air)" stroke-width="5"/>';
    ticks.forEach(function (t) {
      var a = ang(t[0]), p0 = pt(a, 92), p1 = pt(a, t[1] ? 102 : 98), col = t[0] > 0 ? 'var(--on-air)' : 'var(--vu-ink)';
      s += '<line x1="' + p0[0].toFixed(1) + '" y1="' + p0[1].toFixed(1) + '" x2="' + p1[0].toFixed(1) + '" y2="' + p1[1].toFixed(1) + '" style="stroke:' + col + '" stroke-width="1.5"/>';
      if (t[1]) { var q = pt(a, 110); s += '<text x="' + q[0].toFixed(1) + '" y="' + (q[1] + 3).toFixed(1) + '" text-anchor="middle" style="fill:' + col + ';font:600 9px var(--font-label)">' + t[1] + '</text>'; }
    });
    s += '<text x="100" y="92" text-anchor="middle" style="fill:var(--vu-ink);font:800 15px var(--font-label);letter-spacing:.1em">VU</text>';
    s += '<text x="100" y="106" text-anchor="middle" style="fill:var(--vu-ink);font:600 7px var(--font-label);letter-spacing:.18em;opacity:.7">' + (el.dataset.label || 'WOA STUDIO') + '</text>';
    s += '<g class="woa-vu__needle"><line x1="100" y1="128" x2="100" y2="30" style="stroke:var(--vu-ink)" stroke-width="1.6"/></g><circle cx="100" cy="128" r="9" style="fill:var(--surface)"/>';
    el.innerHTML = '<svg viewBox="0 0 200 118" aria-hidden="true">' + s + '</svg>';
    var n = el.querySelector('.woa-vu__needle'), v = { n: n, ang: ang, lv: +(el.dataset.level || -4), cur: -20 };
    function set(db) { n.style.transform = 'rotate(' + ang(Math.max(-20, Math.min(3, db))).toFixed(1) + 'deg)'; }
    v.set = set; set(v.lv); vus.push(v); return v;
  }
  setInterval(function () { if (reduce) return; vus.forEach(function (v) { if (v.n.isConnected) v.set(v.lv + (Math.random() - 0.6) * 5); }); }, 140);
  /* Moderne Pegelansicht: −40…+3 dBFS, RMS-Balken + Peak-Strich + Zahl. */
  var bars = [];
  function barsView(el) {
    var chs = (el.dataset.ch || 'L,R').split(','), lv = +(el.dataset.level || -8), html = '';
    function pos(db) { return Math.max(0, Math.min(100, (db + 40) / 43 * 100)); }
    chs.forEach(function (c) { html += '<span class="woa-bars__ch">' + c + '</span><div class="woa-bars__track" style="--zero:' + pos(0).toFixed(2) + '%"><i class="woa-bars__rms"></i><i class="woa-bars__pk"></i></div><span class="woa-bars__val">−∞</span>'; });
    html += '<div class="woa-bars__scale">' + [-40, -30, -20, -12, -6, -3, 0, 3].map(function (d) { return '<span style="left:' + pos(d).toFixed(1) + '%">' + (d > 0 ? '+' : d < 0 ? '−' : '') + Math.abs(d) + '</span>'; }).join('') + '</div>';
    el.innerHTML = html;
    var tr = el.querySelectorAll('.woa-bars__track'), vals = el.querySelectorAll('.woa-bars__val'), st = [];
    chs.forEach(function () { st.push({ rms: lv, pk: lv + 4 }); });
    function draw() {
      st.forEach(function (s, i) {
        tr[i].style.setProperty('--rms', pos(s.rms).toFixed(1) + '%');
        tr[i].style.setProperty('--pk', pos(s.pk).toFixed(1) + '%');
        vals[i].textContent = (s.pk < 0 ? '−' : '+') + Math.abs(s.pk).toFixed(1);
        vals[i].classList.toggle('is-hot', s.pk > -1);
      });
    }
    var b = { el: el, st: st, lv: lv, draw: draw }; bars.push(b); draw(); return b;
  }
  setInterval(function () {
    if (reduce) return;
    bars.forEach(function (b) {
      b.st.forEach(function (s) {
        var t = b.lv + (Math.random() - 0.6) * 6;
        s.rms += (t - s.rms) * 0.4; var p = s.rms + 3 + Math.random() * 3;
        s.pk = p > s.pk ? p : s.pk - 0.6;
      });
      b.draw();
    });
  }, 120);
  /* Umschalter Analog / Modern / Aus für .woa-levels */
  function levels(el) {
    var btns = el.querySelectorAll('.woa-seg3 button');
    function set(v) { el.setAttribute('data-view', v); btns.forEach(function (b) { b.setAttribute('aria-pressed', b.dataset.view === v); }); }
    btns.forEach(function (b) { b.addEventListener('click', function () { set(b.dataset.view); }); });
    set(el.dataset.view || 'analog');
  }
  /* Tooltip: [data-tip] (+ [data-kbd] "Strg+D", [data-hint]). 500 ms Verzögerung, danach 300 ms „warm“: der nächste erscheint sofort. */
  function kbd(k) { return k ? '<span class="woa-kbd">' + k.split('+').map(function (x) { return '<kbd>' + x + '</kbd>'; }).join('') + '</span>' : ''; }
  function tipHtml(el) {
    return '<span class="woa-tip__row">' + el.dataset.tip + kbd(el.dataset.kbd) + '</span>' + (el.dataset.hint ? '<span class="woa-tip__hint">' + el.dataset.hint + '</span>' : '');
  }
  var tipEl, tipTimer, warmUntil = 0;
  function tips(root) {
    if (!tipEl) { tipEl = document.createElement('div'); tipEl.className = 'woa-tip'; tipEl.setAttribute('role', 'tooltip'); document.body.appendChild(tipEl); }
    root.querySelectorAll('[data-tip]').forEach(function (el) {
      if (!el.getAttribute('aria-label')) el.setAttribute('aria-label', el.dataset.tip + (el.dataset.kbd ? ' (' + el.dataset.kbd + ')' : ''));
      function show() {
        tipEl.innerHTML = tipHtml(el); tipEl.classList.add('is-on');
        var r = el.getBoundingClientRect(), w = tipEl.offsetWidth, h = tipEl.offsetHeight;
        var x = Math.max(4, Math.min(innerWidth - w - 4, r.left + r.width / 2 - w / 2)), y = r.bottom + 6;
        if (y + h > innerHeight - 4) y = r.top - h - 6;
        tipEl.style.left = x + 'px'; tipEl.style.top = y + 'px';
      }
      function enter() { clearTimeout(tipTimer); tipTimer = setTimeout(show, Date.now() < warmUntil ? 0 : 500); }
      function leave() { clearTimeout(tipTimer); if (tipEl.classList.contains('is-on')) warmUntil = Date.now() + 300; tipEl.classList.remove('is-on'); }
      el.addEventListener('mouseenter', enter); el.addEventListener('focus', enter);
      el.addEventListener('mouseleave', leave); el.addEventListener('blur', leave); el.addEventListener('pointerdown', leave);
    });
    root.querySelectorAll('.woa-tip--static[data-tip]').forEach(function (el) { el.innerHTML = tipHtml(el); });
  }
  /* Zahlenfeld: ziehen (hoch/runter, Umschalt = fein), Doppelklick = Standard, Klick = eintippen, Pfeiltasten. */
  var NF = {
    db: function (v) { return (v > 0 ? '+' : v < 0 ? '−' : '') + Math.abs(v).toFixed(1); },
    pan: function (v) { v = Math.round(v); return v === 0 ? 'C' : (v < 0 ? 'L' : 'R') + Math.abs(v); },
    st: function (v) { v = Math.round(v); return (v > 0 ? '+' : v < 0 ? '−' : '') + Math.abs(v); },
    int: function (v) { return String(Math.round(v)); },
    pct: function (v) { return Math.round(v) + ''; }
  };
  function num(el) {
    if (el.classList.contains('is-mixed') || el.dataset.fixed !== undefined) return;
    var min = +(el.dataset.min || -60), max = +(el.dataset.max || 12), step = +(el.dataset.step || 0.1), def = +(el.dataset.default || 0);
    var f = NF[el.dataset.fmt || 'db'], out = el.querySelector('.woa-num__v'), v = +(el.dataset.value || def);
    function set(nv) { v = Math.max(min, Math.min(max, Math.round(nv / step) * step)); out.textContent = f(v); el.dataset.value = v; }
    set(v);
    el.tabIndex = 0;
    var moved = false;
    el.addEventListener('pointerdown', function (e) {
      if (el.classList.contains('is-edit')) return;
      el.setPointerCapture(e.pointerId); moved = false;
      var sy = e.clientY, sv = v;
      function mv(ev) { var d = sy - ev.clientY; if (Math.abs(d) > 2) { moved = true; el.classList.add('is-drag'); } set(sv + d * step * (ev.shiftKey ? 0.2 : 1) * Math.max(1, (max - min) / 200 / step)); }
      function up() { el.classList.remove('is-drag'); el.removeEventListener('pointermove', mv); el.removeEventListener('pointerup', up); }
      el.addEventListener('pointermove', mv); el.addEventListener('pointerup', up);
    });
    el.addEventListener('dblclick', function () { set(def); });
    el.addEventListener('keydown', function (e) {
      var d = { ArrowUp: 1, ArrowDown: -1 }[e.key];
      if (d) { e.preventDefault(); set(v + d * step * (e.shiftKey ? 10 : 1)); }
      if (e.key === 'Enter' && !el.classList.contains('is-edit')) edit();
    });
    function edit() {
      el.classList.add('is-edit'); var inp = document.createElement('input'); inp.value = f(v).replace('−', '-'); out.textContent = ''; out.appendChild(inp); inp.focus(); inp.select();
      function done(ok) { var n = parseFloat(inp.value.replace(',', '.').replace('−', '-')); el.classList.remove('is-edit'); out.textContent = ''; set(ok && !isNaN(n) ? n : v); }
      inp.addEventListener('keydown', function (e) { if (e.key === 'Enter') done(true); if (e.key === 'Escape') done(false); e.stopPropagation(); });
      inp.addEventListener('blur', function () { if (el.classList.contains('is-edit')) done(true); });
    }
  }
  /* Rückgängig-Hinweis: läuft 8 s ab (Fortschrittsstrich). */
  function toast(el) {
    if (reduce || el.dataset.static !== undefined) return;
    var t0 = Date.now(), T = 8000;
    (function step() { var p = 1 - (Date.now() - t0) % T / T; el.style.setProperty('--p', (p * 100).toFixed(1) + '%'); requestAnimationFrame(step); })();
  }
  function rnd(seed) { return function () { seed = (seed * 16807) % 2147483647; return (seed - 1) / 2147483646; }; }
  function wave(cv) {
    var css = getComputedStyle(cv), dpr = window.devicePixelRatio || 1;
    var w = cv.clientWidth || 300, h = cv.clientHeight || 60; cv.width = w * dpr; cv.height = h * dpr;
    var g = cv.getContext('2d'); g.scale(dpr, dpr);
    var r = rnd(+(cv.dataset.seed || 7)), played = +(cv.dataset.played || 0), col = css.getPropertyValue(cv.dataset.color || '--accent').trim();
    var dim = css.getPropertyValue('--lcd-text').trim(), mid = h / 2, step = +(cv.dataset.step || 3), amp = 0.4;
    for (var x = 0; x < w; x += step) {
      amp += (r() - 0.5) * 0.18; amp = Math.max(0.15, Math.min(0.95, amp));
      var a = amp * (0.55 + r() * 0.45) * (h / 2 - 3);
      g.fillStyle = x / w < played ? col : dim; g.globalAlpha = x / w < played ? 1 : 0.38;
      g.fillRect(x, mid - a, Math.max(1, step - 1), a * 2);
    }
    if (played > 0 && played < 1) { g.globalAlpha = 1; g.fillStyle = css.getPropertyValue('--lcd-text').trim(); g.fillRect(Math.round(played * w), 0, 2, h); }
    if (cv.dataset.beats) { g.globalAlpha = .5; g.fillStyle = css.getPropertyValue('--accent-2').trim(); for (var b = 0; b < w; b += +cv.dataset.beats) g.fillRect(b, 0, 1, 5); }
  }
  function init(root) {
    root = root || document;
    root.querySelectorAll('[data-woa-icon]').forEach(function (e) { e.innerHTML = icon(e.dataset.woaIcon) + e.innerHTML; });
    root.querySelectorAll('[data-woa-mark]').forEach(function (e) { e.innerHTML = mark(+e.dataset.woaMark || 26); });
    root.querySelectorAll('[data-woa-mode]').forEach(function (e) { theme(e, e.dataset.woaMode); });
    root.querySelectorAll('.woa-meter').forEach(function (e) { meter(e); });
    root.querySelectorAll('.woa-knob').forEach(knob);
    root.querySelectorAll('.woa-fader').forEach(fader);
    root.querySelectorAll('.woa-clock').forEach(clock);
    root.querySelectorAll('.woa-vu').forEach(vu);
    root.querySelectorAll('.woa-bars').forEach(barsView);
    root.querySelectorAll('.woa-levels').forEach(levels);
    root.querySelectorAll('.woa-num').forEach(num);
    root.querySelectorAll('.woa-toast').forEach(toast);
    tips(root);
    var draw = function () { root.querySelectorAll('canvas[data-woa-wave]').forEach(wave); };
    draw(); new MutationObserver(draw).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    if (document.fonts) document.fonts.ready.then(draw);
  }
  window.WOA = { icon: icon, mark: mark, theme: theme, meter: meter, knob: knob, fader: fader, clock: clock, wave: wave, vu: vu, bars: barsView, levels: levels, num: num, tips: tips, toast: toast, init: init, fmt: FMT };
})();
