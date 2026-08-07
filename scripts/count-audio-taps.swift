#!/usr/bin/env swift
//
// Сколько process tap'ов сейчас живёт в coreaudiod и чьи они.
//
// Инструмент замера для Epic 24: утекают ли наши tap'ы за пределы
// процесса. Считать через чужие приложения («сломалась ли Granola»)
// косвенно и медленно, здесь же видно число до и после.
//
// Порядок замера — на ветке БЕЗ правок Epic 24:
//   1. swift scripts/count-audio-taps.swift          — сколько сейчас
//   2. запустить приложение, начать запись
//   3. убить кнопкой Stop в Xcode (SIGKILL при живом tap'е)
//   4. swift scripts/count-audio-taps.swift          — выросло?
//
// Растёт от прогона к прогону — диагноз подтверждён, tap'ы переживают
// процесс. Не растёт — разбор Epic 24 разворачивать заново.
//
// kAudioHardwarePropertyTapList требует macOS 14.4+. Приватные tap'ы
// (`isPrivate = true`, как у нас) могут быть не видны чужому процессу —
// если список стабильно пуст при заведомо идущей записи, значит видимость
// ограничена владельцем, и этот путь замера не годится.

import CoreAudio
import Foundation

let systemObject = AudioObjectID(kAudioObjectSystemObject)

var listAddress = AudioObjectPropertyAddress(
    mSelector: kAudioHardwarePropertyTapList,
    mScope: kAudioObjectPropertyScopeGlobal,
    mElement: kAudioObjectPropertyElementMain
)

var size: UInt32 = 0
let sizeStatus = AudioObjectGetPropertyDataSize(systemObject, &listAddress, 0, nil, &size)
guard sizeStatus == noErr else {
    FileHandle.standardError.write("список tap'ов недоступен: OSStatus \(sizeStatus)\n".data(using: .utf8)!)
    exit(1)
}

let count = Int(size) / MemoryLayout<AudioObjectID>.size
guard count > 0 else {
    print("tap'ов: 0")
    exit(0)
}

var ids = [AudioObjectID](repeating: kAudioObjectUnknown, count: count)
let readStatus = AudioObjectGetPropertyData(systemObject, &listAddress, 0, nil, &size, &ids)
guard readStatus == noErr else {
    FileHandle.standardError.write("не прочитать список tap'ов: OSStatus \(readStatus)\n".data(using: .utf8)!)
    exit(1)
}

print("tap'ов: \(count)")
for id in ids {
    var uidAddress = AudioObjectPropertyAddress(
        mSelector: kAudioTapPropertyUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var uid: CFString = "" as CFString
    var uidSize = UInt32(MemoryLayout<CFString>.size)
    let status = withUnsafeMutablePointer(to: &uid) { pointer in
        AudioObjectGetPropertyData(id, &uidAddress, 0, nil, &uidSize, pointer)
    }
    print("  \(id): \(status == noErr ? uid as String : "UID недоступен, OSStatus \(status)")")
}
