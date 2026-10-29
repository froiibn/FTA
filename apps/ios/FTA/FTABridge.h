#ifndef FTA_BRIDGE_H
#define FTA_BRIDGE_H
// Import a real C ABI into Swift so calling convention and string ownership are explicit.
// Requests are borrowed for the call; each returned allocation is released exactly once.
char *fta_ios_call(const char *input);
void fta_ios_free(char *pointer);
#endif
