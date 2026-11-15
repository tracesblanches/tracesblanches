package fr.skimap.app.ui.domains

/** Test tags of the area picker. */
object DomainTags {
    const val MAP = "domains_map"
    const val LIST = "domains_list"
    const val SEARCH = "domains_search"

    /** Row of an area of the census. */
    fun row(id: String) = "domain_$id"
}
