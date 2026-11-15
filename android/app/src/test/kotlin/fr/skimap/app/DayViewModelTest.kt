package fr.skimap.app

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import fr.skimap.app.day.Contact
import fr.skimap.app.day.DayBackend
import fr.skimap.app.day.DayStore
import fr.skimap.app.day.DayUi
import fr.skimap.app.day.DayViewModel
import fr.skimap.core.DayPlanView
import fr.skimap.core.DayRequest
import fr.skimap.core.DayStart
import fr.skimap.core.GroupSpec
import fr.skimap.core.MorningView
import fr.skimap.core.groupNew
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import java.io.File
import java.time.LocalDate

/** A morning that records what it was asked. */
class FakeDayBackend(
    private val sites: List<Triple<String, String, String>> = listOf(Triple("/packs/chapelle", "chapelle-des-bois", "Chapelle-des-Bois")),
) : DayBackend {
    val requests = ArrayList<Pair<DayRequest, String?>>()
    val validated = ArrayList<Triple<Int, String?, String>>()

    override fun packs(root: File) = sites

    override fun open(
        packDir: String,
        workDir: String,
    ) = object : DayBackend.Planner {
        override fun morning(
            request: DayRequest,
            groupJson: String?,
        ): MorningView {
            requests += request to groupJson
            return DayFixtures.morning
        }

        override fun validate(
            choice: Int,
            groupJson: String?,
            planDir: String,
        ): DayPlanView {
            validated += Triple(choice, groupJson, planDir)
            return DayFixtures.plan.copy(planDir = planDir)
        }
    }

    override fun starts(siteId: String) = listOf(DayFixtures.LAUS, DayStart("Les Rousses", 6.06, 46.49))

    override fun readPlan(planDir: String): DayPlanView = DayFixtures.plan.copy(planDir = planDir)
}

@OptIn(ExperimentalCoroutinesApi::class)
@RunWith(AndroidJUnit4::class)
class DayViewModelTest {
    @get:Rule
    val tmp = TemporaryFolder()

    @Before
    fun main() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @After
    fun reset() = Dispatchers.resetMain()

    private val ctx = ApplicationProvider.getApplicationContext<Context>()

    private fun store() = DayStore({ ctx.getSharedPreferences("day-test", Context.MODE_PRIVATE) }, { tmp.root })

    private fun vm(
        backend: DayBackend,
        store: DayStore = store(),
    ) = DayViewModel(
        backend,
        store,
        { tmp.root },
        { tmp.newFolder() },
        "chapelle-des-bois",
        LocalDate.of(2026, 11, 16),
        Dispatchers.Unconfined,
    )

    @Test
    fun theMorningFollowsTheChoicesAndTheGroup() =
        runTest {
            ctx
                .getSharedPreferences("day-test", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
            val b = FakeDayBackend()
            val v = vm(b)
            val s = v.state.value as DayUi.Ready
            // Defaults: the first start, 2 h, classic; the groups « Seul » and « En famille ».
            assertEquals(listOf("Seul", "En famille"), s.groups.map { it.spec.name })
            assertEquals("Seul", s.group!!.spec.name)
            assertEquals(DayFixtures.morning, s.morning)
            assertEquals(1, b.requests.size)
            assertEquals("2 h, classique", b.requests[0].first.label)
            assertEquals(120.0, b.requests[0].first.budgetMin, 0.0)
            // Each choice recomputes the loops.
            v.setInputs { it.copy(budgetMin = 90, skating = true, returnBefore = "16:00", easyOnly = true) }
            val r = b.requests.last().first
            assertEquals("1 h 30, skating, vertes et bleues, retour avant 16 h 00", r.label)
            assertEquals("easy", r.maxDifficulty)
            assertEquals("16:00", r.returnBefore)
            // The group's model goes to the core with the request (C11).
            val fam = s.groups[1]
            v.selectGroup(fam)
            assertEquals(fam.modelJson, b.requests.last().second)
            assertEquals("En famille", store().selectedGroup())
        }

    @Test
    fun validateWritesThePlanOfTheChosenLoopForTheGroup() =
        runTest {
            ctx
                .getSharedPreferences("day-test", Context.MODE_PRIVATE)
                .edit()
                .clear()
                .commit()
            val b = FakeDayBackend()
            val st = store()
            val v = vm(b, st)
            v.choose(1)
            v.setContact(Contact("Marie", "0612345678"))
            val p = v.validate().getOrThrow()
            assertEquals(1, b.validated.single().first)
            assertTrue(File(p.planDir).isDirectory)
            assertTrue(p.planDir.endsWith("plans/2026-11-16-1"))
            assertEquals("Seul", File(p.planDir, DayStore.GROUP_FILE).readText())
            assertEquals(p, v.validated.value)
            assertEquals(Contact("Marie", "0612345678"), st.contact())
            // A second plan the same day gets its own directory.
            assertTrue(
                v
                    .validate()
                    .getOrThrow()
                    .planDir
                    .endsWith("-2"),
            )
        }

    @Test
    fun noPackOfASupportedSite() =
        runTest {
            val v = vm(FakeDayBackend(emptyList()))
            assertTrue(v.state.value is DayUi.Unavailable)
        }

    @Test
    fun theGroupLearnsFromTheOutingOnce() {
        // Real core (host library): a family model and an observation of one outing.
        ctx
            .getSharedPreferences("day-test", Context.MODE_PRIVATE)
            .edit()
            .clear()
            .commit()
        val st = store()
        val g = groupNew(GroupSpec("Famille test", 2u, byteArrayOf(6, 9), false, "intermediate"))
        st.putGroup(g)
        val dir = tmp.newFolder("plan")
        st.setPlanGroup(dir.absolutePath, "Famille test")
        File(dir, DayStore.OBSERVATION_FILE).writeText(
            """{"model_s":[1200.0,2400.0,600.0],"moving_s":[2400.0,4100.0,900.0],"progress_m":11000.0,""" +
                """"travelled_m":11900.0,"stops_s":[300.0,420.0,240.0],"elapsed_s":8400.0}""",
        )
        val learned = st.learnFrom(dir.absolutePath)!!
        assertEquals(1u, learned.outings)
        assertEquals("une sortie apprise", learned.learned)
        assertEquals(1u, st.groups().first { it.spec.name == "Famille test" }.outings)
        assertNull(st.learnFrom(dir.absolutePath))
    }
}
