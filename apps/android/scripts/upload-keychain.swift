// A local macOS release tool, not app code. Secrets travel only through pipes and the user's login Keychain.
import Foundation
import Security
let query:[String:Any]=[kSecClass as String:kSecClassGenericPassword,kSecAttrService as String:"FTA.Android.UploadKey.v1",kSecAttrAccount as String:"app.fta.android"]
let action=CommandLine.arguments.dropFirst().first ?? ""
if action == "read" {
    var request=query;request[kSecReturnData as String]=true;request[kSecMatchLimit as String]=kSecMatchLimitOne
    var value:CFTypeRef?;let status=SecItemCopyMatching(request as CFDictionary,&value)
    guard status == errSecSuccess,let data=value as? Data else { fputs("Upload password lookup status: \(status)\n",stderr);exit(status == errSecItemNotFound ? 4:1) }
    FileHandle.standardOutput.write(data)
} else if action == "store" {
    let secret=FileHandle.standardInput.readDataToEndOfFile();guard secret.count>=32 else {exit(2)}
    var request=query;request[kSecValueData as String]=secret;request[kSecAttrSynchronizable as String]=false;request[kSecAttrLabel as String]="FTA Android upload password (local candidate)"
    // Refuse duplicate items: another established upload identity must never be silently rotated.
    let status=SecItemAdd(request as CFDictionary,nil);guard status==errSecSuccess else {fputs("Upload password storage status: \(status)\n",stderr);exit(1)}
} else {fputs("Expected read or store\n",stderr);exit(2)}
