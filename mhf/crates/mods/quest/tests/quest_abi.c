#include "mhf_game.h"
#include <stddef.h>

_Static_assert(sizeof(QuestSnapshot) == 2 * sizeof(size_t), "QuestSnapshot layout");
_Static_assert(offsetof(QuestSnapshot, hunter_initialized) == 2, "QuestSnapshot hunter flag");
_Static_assert(offsetof(QuestSnapshot, quest_size) == sizeof(size_t), "QuestSnapshot quest_size");
_Static_assert(sizeof(QuestMonsterSpawn) == 20, "QuestMonsterSpawn layout");
_Static_assert(offsetof(QuestMonsterSpawn, position) == 4, "QuestMonsterSpawn position");
_Static_assert(offsetof(QuestMonsterSpawn, yaw) == 16, "QuestMonsterSpawn yaw");
_Static_assert(sizeof(QuestSpawnOffset) == 4, "QuestSpawnOffset layout");
_Static_assert(sizeof(QuestTable) == 3 * sizeof(void *), "QuestTable layout");
_Static_assert(sizeof(QuestControlTable) == 8 * sizeof(void *), "QuestControlTable layout");
_Static_assert(sizeof(QuestLaunchTable) == 3 * sizeof(void *), "QuestLaunchTable layout");
_Static_assert(offsetof(QuestControlTable, vtable.prepare_monster_spawn) == 6 * sizeof(void *),
               "QuestControlTable prepare_monster_spawn");
