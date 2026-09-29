# AGENTS.md — apps/macos/Sources/App

Единственный сетевой слой Swift (URLSession) и загрузка моделей. Сторы
настроек. Общие границы — в `apps/macos/AGENTS.md`; здесь локальные
ловушки.

## Загрузка моделей

- `GigaamModelCatalog` обязан совпадать с `scripts/fetch-gigaam-models.sh`
  (URL, размеры, маркер `<export>/<file>`, который пишется **после** файла):
  разойдётся — скрипт скачает ~230 МБ заново. Две дороги к одному каталогу
  обязаны оставлять одинаковый след. `WhisperModelId` паритет держит с
  `apps/macos/Scripts/download-stt-model.sh` — по именам файлов
  (`ggml-base.bin`, `ggml-small.bin`, `ggml-large-v3-turbo.bin`) и URL;
  размеров и маркеров у того скрипта нет.
- Скачивание идёт в `.partial` и переезжает на место; частичный файл
  удаляется всегда (он неотличим от целого), готовый заново не тянется.
- `FileDownloading` — транспорт (`.partial`, прогресс, HTTP-коды), чтобы
  правда о скачивании не размножалась по движкам; `WhisperDownloading` и
  `GigaamDownloading` сидят на нём.

## Потоки

`offMainThread` (`BlockingCore.swift`) использует `DispatchQueue.global`,
**не** `Task.detached`: кооперативный пул рассчитан на неблокирующие
задачи. Блокирующие вызовы ядра (сеть) идут только через него.

## Настройки и первый запуск

- `ProviderSettingsStore` сохраняет **только** `postCallRecognizer`;
  неизвестное сохранённое значение → `.auto`.
- `FirstRunModelBootstrap` тихо деградирует до mock — не ошибка, но и не
  работающий распознаватель.
